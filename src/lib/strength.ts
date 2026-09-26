// Rough master-password strength estimate for the create-vault screen.
// Not a security boundary: Argon2id does the real work. This only nudges
// people away from short or obviously patterned passwords.

export interface Strength {
  score: 0 | 1 | 2 | 3 | 4;
  label: string;
  bits: number;
}

const COMMON = ["password", "123456", "qwerty", "letmein", "admin", "welcome", "iloveyou", "sshvault"];

export function estimate(pw: string): Strength {
  if (!pw) return { score: 0, label: "", bits: 0 };
  let pool = 0;
  if (/[a-z]/.test(pw)) pool += 26;
  if (/[A-Z]/.test(pw)) pool += 26;
  if (/[0-9]/.test(pw)) pool += 10;
  if (/[^a-zA-Z0-9]/.test(pw)) pool += 33;
  // Count distinct characters so "aaaaaaaaaaaa" doesn't score like random text.
  const unique = new Set(pw).size;
  const effectiveLen = Math.min(pw.length, unique * 2);
  let bits = effectiveLen * Math.log2(Math.max(pool, 2));
  const lower = pw.toLowerCase();
  if (COMMON.some((c) => lower.includes(c))) bits = Math.min(bits, 20);
  if (/^(.)\1*$/.test(pw) || /^(0123|1234|abcd)/i.test(pw)) bits = Math.min(bits, 15);

  const score = bits < 28 ? 0 : bits < 40 ? 1 : bits < 60 ? 2 : bits < 80 ? 3 : 4;
  const label = ["Very weak", "Weak", "Fair", "Strong", "Very strong"][score];
  return { score: score as Strength["score"], label, bits: Math.round(bits) };
}
