use eyre::{Context, Result, bail};
use reqwest::{StatusCode, Url, blocking::Response};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

const MAX_RETRIES: u32 = 5;

pub(crate) struct Downloader {
    client: reqwest::blocking::Client,
}

impl Downloader {
    pub(crate) fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .https_only(true)
            .user_agent(concat!("tempoup/", env!("CARGO_PKG_VERSION")))
            .build()
            .wrap_err("failed to create HTTP client")?;
        Ok(Self { client })
    }

    fn send(&self, url: &str) -> Result<Response> {
        let parsed = Url::parse(url).wrap_err_with(|| format!("invalid URL {url}"))?;
        let attempts = MAX_RETRIES + 1;
        let github_token = is_github_api_url(&parsed).then(github_token).flatten();

        for attempt in 1..=attempts {
            let mut request = self.client.get(parsed.clone());
            if let Some(token) = github_token.as_deref() {
                request = request.bearer_auth(token);
            }

            match request.send() {
                Ok(response)
                    if response.status().is_success()
                        || !is_retryable_status(response.status()) =>
                {
                    return Ok(response);
                }
                Ok(response) if attempt == attempts => return Ok(response),
                Err(error) if attempt == attempts => {
                    return Err(error).wrap_err_with(|| format!("failed to GET {url}"));
                }
                Ok(_) | Err(_) => {
                    thread::sleep(Duration::from_millis(250 * u64::from(attempt)));
                }
            }
        }

        unreachable!("the retry loop always returns")
    }

    fn send_ok(&self, url: &str) -> Result<Response> {
        let response = self.send(url)?;
        if !response.status().is_success() {
            bail!("failed to download {url}: HTTP {}", response.status());
        }
        Ok(response)
    }

    pub(crate) fn download_to_file(&self, url: &str, path: &Path) -> Result<()> {
        let mut response = self.send_ok(url)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        std::io::copy(&mut response, &mut file)?;
        file.flush()?;
        Ok(())
    }

    pub(crate) fn download_to_string(&self, url: &str) -> Result<String> {
        self.send_ok(url)?
            .text()
            .wrap_err("failed to read response body")
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 403 | 408 | 429 | 500 | 502 | 503 | 504)
}

fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .or_else(|| {
            let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty())
        })
}

fn is_github_api_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.github.com"))
        && url.port_or_known_default() == Some(443)
}

pub(crate) fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn extract_tar_gz_file(
    archive_path: &Path,
    destination: &Path,
    expected_name: &str,
) -> Result<()> {
    fs::create_dir_all(destination)?;
    let file = File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut extracted = false;
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() != Path::new(expected_name) {
            continue;
        }
        if !entry.header().entry_type().is_file() || extracted {
            bail!("archive contains an invalid {expected_name} entry");
        }
        entry.unpack(destination.join(expected_name))?;
        extracted = true;
    }
    if !extracted {
        bail!("archive does not contain {expected_name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses_are_limited() {
        for code in [403, 408, 429, 500, 502, 503, 504] {
            assert!(is_retryable_status(StatusCode::from_u16(code).unwrap()));
        }
        for code in [200, 400, 401, 404] {
            assert!(!is_retryable_status(StatusCode::from_u16(code).unwrap()));
        }
    }

    #[test]
    fn tokens_are_only_sent_to_the_github_api() {
        let matches = |url: &str| is_github_api_url(&Url::parse(url).unwrap());
        assert!(matches(
            "https://api.github.com/repos/tempoxyz/tempo/releases"
        ));
        assert!(!matches("https://api.github.com.evil.example/"));
        assert!(!matches("https://github.com/tempoxyz/tempo/releases"));
        assert!(!matches("http://api.github.com/"));
    }

    #[test]
    fn computes_sha256() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input");
        fs::write(&path, b"tempo").unwrap();
        assert_eq!(
            compute_sha256(&path).unwrap(),
            "8d6546721a1d106cf8d27f7326ebae7e83c1592aeb7479b8f7ec9d8d700d464f"
        );
    }

    #[test]
    fn extracts_only_the_expected_regular_file() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("archive.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            File::create(&archive_path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let contents = b"binary";
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "tempoup", &contents[..])
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();

        let extracted = directory.path().join("extracted");
        extract_tar_gz_file(&archive_path, &extracted, "tempoup").unwrap();
        assert_eq!(fs::read(extracted.join("tempoup")).unwrap(), contents);
        assert!(extract_tar_gz_file(&archive_path, &extracted, "other").is_err());
    }
}
