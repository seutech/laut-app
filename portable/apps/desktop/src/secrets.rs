//! Native credential-vault access. Secrets never enter preferences, projects or logs.
use anyhow::{Context, Result, ensure};
use zeroize::Zeroizing;

const SERVICE: &str = "com.seutech.laut.external-ai";

fn profile(value: &str) -> Result<&str> {
    ensure!(
        !value.is_empty()
            && value.len() <= 64
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')),
        "Invalid credential profile"
    );
    Ok(value)
}

fn entry(profile_id: &str) -> Result<keyring::Entry> {
    let profile_id = profile(profile_id)?;
    keyring::Entry::new(SERVICE, profile_id)
        .context("Operating-system credential vault unavailable")
}

pub fn available() -> Result<()> {
    keyring::Entry::store_status()
        .as_ref()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("Operating-system credential vault unavailable: {error}"))
}

pub fn save(profile_id: &str, secret: &str) -> Result<()> {
    ensure!(
        (8..=4096).contains(&secret.len())
            && secret.trim().len() == secret.len()
            && !secret.chars().any(|c| c.is_control()),
        "Enter a valid API key"
    );
    entry(profile_id)?
        .set_password(secret)
        .context("API key could not be saved in the operating-system credential vault")
}

pub fn load(profile_id: &str) -> Result<Zeroizing<String>> {
    let secret = entry(profile_id)?
        .get_password()
        .context("No API key is stored for this profile")?;
    ensure!(
        (8..=4096).contains(&secret.len()),
        "Stored API key is invalid"
    );
    Ok(Zeroizing::new(secret))
}

pub fn delete(profile_id: &str) -> Result<()> {
    entry(profile_id)?
        .delete_credential()
        .context("Stored API key could not be deleted")
}

#[cfg(test)]
mod tests {
    use super::profile;

    #[test]
    fn credential_profile_cannot_become_a_path_or_command() {
        assert_eq!(profile("openai-default").unwrap(), "openai-default");
        for value in ["", "../secret", "has space", "line\nbreak", "a/b"] {
            assert!(profile(value).is_err(), "{value:?}");
        }
    }
}
