import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
import {seedDownloadReview} from './download-review-fixture.mjs';

const executable = process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('Windows first-run setup reviews download sizes and waits for approval', {skip: !existsSync(executable)}, async () => {
  const server = await preview({root: fileURLToPath(new URL('..', import.meta.url)), preview: {host: '127.0.0.1', port: 0, open: false}});
  const browser = await chromium.launch({executablePath: executable, headless: true});
  try {
    const page = await browser.newPage({viewport: {width: 1300, height: 950}});
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(seedVoiceHistory, 0);
    await page.addInitScript(() => {
      if (window.self !== window.top) return;
      const base = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (command === 'system_summary') return {os: 'windows', arch: 'x86_64', logicalCpus: 24, totalMemoryBytes: 64 * 1024 ** 3};
        if (command === 'get_setup_plan') return {
          supported: true, reason: 'Tools are matched to this computer.',
          hardware: {os: 'windows', arch: 'x86_64', totalRamBytes: 64 * 1024 ** 3, freeRamBytes: 40 * 1024 ** 3, physicalCores: 12, nvidia: []},
          osVersion: '11', freeStorageBytes: 1024 ** 4,
          routing: {tier: 'cpu-high', downloadBytes: 120000000, downloads: [], warnings: []},
          features: [{id: 'transcription', title: 'Transcription', ready: false, canInstall: true, message: 'Ready to download the tools selected for this computer.', downloads: []}]
        };
        if (command === 'get_voice_setup') return {state: 'idle', stage: 'Set up Amharic transcription and dictation on this computer.', downloadedBytes: 0, totalBytes: 120000000, canInstall: true};
        if (command === 'start_voice_setup') return;
        return base(command, args);
      };
    });
    await page.addInitScript(seedDownloadReview, true);
    await page.goto(server.resolvedUrls.local[0]);
    await page.getByRole('dialog', {name: 'Set up your voice tools', exact: true}).getByRole('button', {name: 'Review downloads'}).click();
    await page.getByRole('region', {name: 'Automatic model setup'}).getByText('Windows computer', {exact: true}).waitFor();
    const setup = page.getByRole('region', {name: 'Voice tools setup'});
    await setup.getByText('On your computer', {exact: true}).waitFor();
    assert.equal(await page.evaluate(() => window.downloadRequests.length), 0);
    await setup.getByRole('button', {name: 'Download voice tools', exact: true}).click();
    const review = page.getByRole('dialog', {name: 'Review your download'});
    await review.getByText('Maximum download', {exact: true}).waitFor();
    assert.equal(await page.evaluate(() => window.downloadRequests.length), 0);
    await review.getByRole('button', {name: 'Not now', exact: true}).click();
    assert.equal(await page.evaluate(() => window.downloadRequests.length), 0);
    await setup.getByRole('button', {name: 'Download voice tools', exact: true}).click();
    await review.getByRole('button', {name: 'Download and set up', exact: true}).click();
    assert.deepEqual(await page.evaluate(() => window.downloadRequests), [{command: 'start_voice_setup', args: {approvedPlan: 'quote-voice-tools', acceptedConsent: false}}]);
    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
    await new Promise(resolve => server.httpServer.close(resolve));
  }
});
