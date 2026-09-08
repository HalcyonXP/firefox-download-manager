// Detection policy, not an anonymity guarantee. Never return matched values.
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/gu;
const PROFILE =
  /(?:[A-Z]:[\\/]+Users[\\/]+|(?<![A-Za-z0-9:/.-])\/(?:home|Users)\/)([^\\/\r\n"'<>]+)/giu;
const SYNTHETIC_PROFILES = new Set(["Example", "Example User", "you"]);
const SECRET =
  /(?:gh[pousr]_[A-Za-z0-9]{25,}|github_pat_[A-Za-z0-9_]{25,}|AKIA[A-Z0-9]{16}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)/u;

export function allowedEmail(value, history = false) {
  if (typeof value !== "string" || value.length === 0) return false;
  const domain = value.slice(value.lastIndexOf("@") + 1).toLowerCase();
  if (domain === "users.noreply.github.com" || value === "noreply@github.com") return true;
  if (history) return false;
  // Fixed native-extension principal, not a personal contact address.
  if (value === "download-manager@halcyonxp.local") return true;
  return (
    /(?:^|\.)(?:test|invalid|example)$/u.test(domain) ||
    /^(?:.*\.)?example\.(?:com|org|net)$/u.test(domain)
  );
}

export function inspectText(text, { history = false } = {}) {
  const findings = [];
  for (const [index, line] of text.split(/\r?\n/u).entries()) {
    if ([...line.matchAll(EMAIL)].some(([value]) => !allowedEmail(value, history)))
      findings.push({ line: index + 1, category: "non-public-email" });
    if ([...line.matchAll(PROFILE)].some((match) => !SYNTHETIC_PROFILES.has(match[1])))
      findings.push({ line: index + 1, category: "personal-profile-path" });
    if (SECRET.test(line)) findings.push({ line: index + 1, category: "credential-pattern" });
  }
  return findings;
}

export function sensitivePath(path) {
  return (
    (/(?:^|\/)(?:\.env(?:\.|$)|state\/|artifacts\/|\.playwright-cli\/|node_modules\/|target\/)/u.test(
      path,
    ) &&
      !path.endsWith(".env.example")) ||
    /\.(?:pfx|p12|pem|key|log|part|xpi|exe|zip)$/iu.test(path)
  );
}
