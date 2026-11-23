// Resolve only the two shipped product layouts. Never turn a manifest string into a path.
export function candidateLayout(product) {
  if (product === 'Grove')
    return {
      product,
      app: 'Grove.app',
      binary: 'grove',
      portable: 'grove-export.json',
    };
  if (product === 'Moment Grove')
    return {
      product,
      app: 'Moment Grove.app',
      binary: 'moment-grove',
      portable: 'moment-grove-export.json',
    };
  throw Error('Unknown candidate product');
}

export function candidateBinarySha256(manifest) {
  if (manifest.binarySha256 !== undefined) return manifest.binarySha256;
  if (manifest.appVersion === '0.1.0' && !manifest.product)
    return manifest.sourceAppExecutableSha256;
}

export function batchLayout(manifest) {
  // Prepared 0.1.0 batches predate the product field. Require their original executable fingerprint.
  const legacyBinary = [...(manifest.appFiles ?? []), ...(manifest.files ?? [])].some(
    (f) =>
      ['Contents/MacOS/moment-grove', 'Moment Grove QA.app/Contents/MacOS/moment-grove'].includes(
        f.path,
      ) &&
      /^[a-f0-9]{64}$/.test(f.sha256) &&
      f.sha256 === candidateBinarySha256(manifest),
  );
  return candidateLayout(
    manifest.product ??
      (manifest.appVersion === '0.1.0' && legacyBinary ? 'Moment Grove' : undefined),
  );
}

export function exportLayout(directory) {
  const m = /^(Grove|Moment-Grove)-(?:Watermark-)?\d+-[a-f0-9]{8}$/.exec(directory);
  return m ? candidateLayout(m[1] === 'Grove' ? 'Grove' : 'Moment Grove') : null;
}
