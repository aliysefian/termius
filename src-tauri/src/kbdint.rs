//! Answering a server's keyboard-interactive prompts.
//!
//! Many servers ask for a password this way, but the same mechanism carries one-time codes, push approvals and other
//! questions. The saved password is for password questions only: sent to a "Verification code:" prompt it would be
//! wrong (and costs an attempt), and it would hand the password to whatever the server chose to ask.

/// What a prompt is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    Password,
    /// A one-time code from an authenticator app (see `totp`).
    Code,
    Other,
}

/// A prompt that reads as a password question is one, whatever else it mentions ("Password (MFA):").
pub fn classify(prompt: &str, echo: bool) -> Ask {
    if is_password_prompt(prompt, echo) {
        Ask::Password
    } else if crate::totp::is_code_prompt(prompt) {
        Ask::Code
    } else {
        Ask::Other
    }
}

/// What to send for one prompt: the password if it is asking for one, otherwise nothing. Codes are answered by the
/// caller (it has to wait for a fresh one), and only when two-factor codes are switched on.
pub fn answer(prompt: &str, echo: bool, password: &str) -> String {
    match classify(prompt, echo) {
        Ask::Password => password.to_string(),
        Ask::Code | Ask::Other => String::new(),
    }
}

pub fn is_password_prompt(prompt: &str, echo: bool) -> bool {
    // A password is never typed in view; a prompt that echoes is asking for something else (a user name, a code).
    if echo {
        return false;
    }
    let p = prompt.to_lowercase();
    const NOT_A_PASSWORD: [&str; 11] = ["code", "otp", "token", "one-time", "one time", "passcode", "verification", "authenticator", "2fa", "pin", "duo"];
    if NOT_A_PASSWORD.iter().any(|w| p.contains(w)) {
        return false;
    }
    p.contains("password") || p.contains("passphrase")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_prompts_get_the_password() {
        for p in ["Password: ", "user@host's password:", "Password for alice:", "Enter your password"] {
            assert_eq!(answer(p, false, "s3cret"), "s3cret", "{p}");
        }
    }

    #[test]
    fn other_questions_never_get_it() {
        for p in ["Verification code: ", "One-time password (OTP):", "Passcode or option (1-3):", "Enter token", "Duo passcode or option:", "Authenticator code", "PIN:", "Username: ", "Do you want to continue?", ""] {
            assert_eq!(answer(p, false, "s3cret"), "", "{p:?}");
        }
        assert_eq!(answer("Password: ", true, "s3cret"), "", "a prompt that echoes is not a password");
    }

    #[test]
    fn tells_passwords_codes_and_the_rest_apart() {
        assert_eq!(classify("Password: ", false), Ask::Password);
        assert_eq!(classify("Verification code: ", false), Ask::Code);
        assert_eq!(classify("Verification code: ", true), Ask::Code, "a code may be shown as it is typed");
        assert_eq!(classify("One-time password (OTP):", false), Ask::Code);
        assert_eq!(classify("Duo passcode or option (1-3):", false), Ask::Other);
        assert_eq!(classify("Username: ", true), Ask::Other);
        assert_eq!(classify("Password (MFA): ", false), Ask::Password, "a password question gets the password, never a code");
    }
}
