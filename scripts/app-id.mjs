// Single source of truth for the application identifier.
//
// The same value reaches the Android applicationId, the iOS/macOS bundle
// identifier and the GTK application id. Keeping it in one module means the
// release artifacts cannot disagree about who published them.

export const APP_ID = 'io.github.seekdeeply.batchhash';
export const APP_ID_SEGMENTS = APP_ID.split('.');
export const APP_NAME = 'BatchHash';
