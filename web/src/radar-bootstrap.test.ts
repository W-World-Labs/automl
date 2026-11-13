import { describe, expect, it } from 'vitest';
// Vite raw import — keeps this under browser/tsconfig without @types/node.
import shell from '../index.html?raw';

const FORMAL_CODE = 'W-W-AUTOML-C06A4011';

describe('W-Radar platform bootstrap shell', () => {
  it('web/index.html carries formal code + platform/bootstrap.js', () => {
    expect(shell).toContain('platform/bootstrap.js');
    expect(shell).toContain(`data-code="${FORMAL_CODE}"`);
    expect(shell).toContain('data-project-id="w-automl"');
    expect(shell).toContain('data-workspace-id="ws_platform"');
    expect(shell).toContain('data-client-kind="w-radar-platform"');
    expect(shell).toContain('radar.myfastools.com/platform/bootstrap.js');
  });
});
