//! Answering a server's keyboard-interactive prompts.
//!
//! Many servers ask for a password this way, but the same mechanism carries one-time codes, push approvals and other
//! questions. The saved password is for password questions only: sent to a "Verification code:" prompt it would be
//! wrong (and costs an attempt), and it would hand the password to whatever the server chose to ask.

/// What to send for one prompt: the password if it is asking for one, otherwise nothing.
pub fn answer(prompt: &str, echo: bool, password: &str) -> String {
    if is_password_prompt(prompt, echo) {
        password.to_string()
    } else {
        String::new()
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
}
