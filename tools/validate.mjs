// Validates glTF and GLB files with the Khronos glTF-Validator, a local
// oracle that is never a dependency (npm's `gltf-validator`, installed
// anywhere):
//
//   GLTF_VALIDATOR=<dir>/node_modules/gltf-validator node tools/validate.mjs <files or directories>
//
// Prints one line per file: its errors, warnings and infos, and the codes
// of its errors and warnings. Exits 1 when any file has an error.
import { createRequire } from 'node:module';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';

const validator = createRequire(import.meta.url)(process.env.GLTF_VALIDATOR ?? 'gltf-validator');

function files(path) {
  if (!statSync(path).isDirectory()) return [path];
  return readdirSync(path).sort().flatMap((name) => files(join(path, name)))
    .filter((file) => /\.(gltf|glb)$/i.test(file));
}

let failed = false;
for (const file of process.argv.slice(2).flatMap(files)) {
  const report = await validator.validateBytes(new Uint8Array(readFileSync(file)), {
    uri: file,
    maxIssues: 0,
    externalResourceFunction: (uri) =>
      Promise.resolve(new Uint8Array(readFileSync(join(dirname(file), decodeURIComponent(uri))))),
  });
  const { numErrors, numWarnings, numInfos, messages } = report.issues;
  const codes = [...new Set(messages.filter((m) => m.severity <= 1).map((m) => m.code))];
  console.log(`${file}: ${numErrors} errors, ${numWarnings} warnings, ${numInfos} infos ${codes.join(' ')}`);
  failed ||= numErrors > 0;
}
process.exit(failed ? 1 : 0);
