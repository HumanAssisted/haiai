/** Strict-safe staging for the committed cross-language media verifier. */

import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
} from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

export const VERIFIER_AGENT_PASSWORD = 'MediaFixtureVerifierPass!123';

export interface MediaSignerFixture {
  signer_id: string;
  algorithm: string;
  public_key_file: string;
  verifier_agent_dir: string;
}

export interface StagedMediaVerifier {
  configPath: string;
  tmpDir: string;
  keyDir: string;
  verificationKeyDir: string;
  jacsId: string;
  signer: MediaSignerFixture;
}

function copyVerifierTree(src: string, dst: string, inAgentDir = false): void {
  mkdirSync(dst, { recursive: true });
  for (const name of readdirSync(src)) {
    const srcPath = join(src, name);
    const isDirectory = statSync(srcPath).isDirectory();
    // Git stores `{id}_{version}.json` because `:` is illegal on Windows.
    // Restore only agent-document filenames; preserve `public_keys`.
    const newName = !isDirectory && inAgentDir ? name.replace(/_/g, ':') : name;
    const dstPath = join(dst, newName);
    if (isDirectory) {
      copyVerifierTree(srcPath, dstPath, inAgentDir || name === 'agent');
    } else {
      copyFileSync(srcPath, dstPath);
    }
  }
}

export function stageSignedMediaVerifier(
  repoRoot: string,
  prefix: string,
): StagedMediaVerifier {
  if (
    process.env.JACS_ALLOW_UNSIGNED_AGENT_CONFIG ||
    process.env.JACS_ALLOW_LEGACY_SIGNATURE_CONTENT
  ) {
    throw new Error('native media tests must not enable unsigned or legacy JACS compatibility');
  }

  process.env.JACS_PRIVATE_KEY_PASSWORD = VERIFIER_AGENT_PASSWORD;
  const mediaDir = join(repoRoot, 'fixtures', 'media');
  const signer = JSON.parse(
    readFileSync(join(mediaDir, 'SIGNER.json'), 'utf-8'),
  ) as MediaSignerFixture;

  const tmpDir = realpathSync(mkdtempSync(join(tmpdir(), prefix)));
  copyVerifierTree(join(repoRoot, signer.verifier_agent_dir), tmpDir);

  const configPath = join(tmpDir, 'jacs.config.json');
  const config = JSON.parse(readFileSync(configPath, 'utf-8')) as Record<string, unknown>;
  const signature = config.jacsSignature as Record<string, unknown> | undefined;
  if (signature?.signatureContentVersion !== 'jacs-signature-v2') {
    throw new Error('committed media verifier config must use jacs-signature-v2');
  }
  if (config.jacs_default_storage !== 'fs') {
    throw new Error('committed media verifier must keep local signer storage on fs');
  }

  const verificationKeyDir = join(tmpDir, 'verification-keys');
  mkdirSync(verificationKeyDir);
  copyFileSync(
    join(repoRoot, signer.public_key_file),
    join(verificationKeyDir, `${signer.signer_id}.public.pem`),
  );

  return {
    configPath,
    tmpDir,
    keyDir: join(tmpDir, config.jacs_key_directory as string),
    verificationKeyDir,
    jacsId: (config.jacs_agent_id_and_version as string).split(':')[0],
    signer,
  };
}
