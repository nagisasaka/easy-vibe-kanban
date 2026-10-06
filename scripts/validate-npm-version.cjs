// Legacy npm version parser retained for old inputs; LVK distributes Docker images.
const pattern =
  /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(beta|easy|lvk)\.(0|[1-9][0-9]*))?$/;

function validateNpmVersion(version) {
  return (
    typeof version === 'string' &&
    version === version.trim() &&
    pattern.test(version)
  );
}

if (require.main === module && !validateNpmVersion(process.env.VERSION)) {
  console.error(
    'Version must be stable (0.1.44), beta (0.1.44-beta.1), or LVK (0.1.44-lvk.1), without whitespace or leading zeroes.'
  );
  process.exitCode = 1;
}

module.exports = { validateNpmVersion };
