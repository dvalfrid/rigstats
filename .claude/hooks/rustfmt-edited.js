// PostToolUse (Edit/Write): rustfmt the .rs file that was just changed, so
// formatting never piles up until commit time (lefthook only checks).
const { execFileSync } = require('child_process');

let input = '';
process.stdin.on('data', (d) => (input += d));
process.stdin.on('end', () => {
  let file = '';
  try {
    file = JSON.parse(input).tool_input?.file_path ?? '';
  } catch {
    process.exit(0);
  }
  if (!file.endsWith('.rs')) process.exit(0);
  try {
    execFileSync('rustfmt', ['--edition', '2021', file], { stdio: ['ignore', 'ignore', 'pipe'], timeout: 30000 });
  } catch (e) {
    // A syntax error mid-edit is normal; say so but don't block.
    process.stderr.write(`rustfmt skipped ${file}: ${e.stderr?.toString().split('\n')[0] ?? e.message}\n`);
  }
  process.exit(0);
});
