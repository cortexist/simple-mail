/**
 * Attachment safety classification.
 *
 * Goal: spot files a user could double-click into trouble — native binaries,
 * scripts that auto-execute when opened, installer packages — plus archives
 * that commonly carry the above. The check is deliberately conservative
 * (extension + MIME type, no content sniffing): the cost of a false positive
 * is one extra click via Save As, while a false negative could detonate code.
 */

// Extensions that launch code when opened on at least one major OS.
// Stored without the leading dot, lowercased.
const EXECUTABLE_EXTENSIONS: ReadonlySet<string> = new Set([
  // Windows native + scripting hosts
  'exe', 'com', 'scr', 'pif', 'cpl', 'dll', 'sys', 'drv',
  'bat', 'cmd', 'ps1', 'ps2', 'psc1', 'psc2', 'psm1', 'psd1',
  'vbs', 'vbe', 'js', 'jse', 'wsf', 'wsh', 'hta',
  'msi', 'msp', 'mst', 'reg', 'inf', 'scf', 'lnk', 'url',
  'gadget', 'application', 'appref-ms',
  // macOS
  'app', 'command', 'tool', 'pkg', 'dmg', 'mpkg',
  'scpt', 'scptd', 'workflow', 'action', 'osascript',
  // Linux / cross-Unix
  'sh', 'bash', 'csh', 'ksh', 'zsh', 'fish',
  'run', 'bin', 'out', 'deb', 'rpm', 'appimage',
  // Cross-platform runtimes that auto-execute on common setups
  'jar', 'class', 'apk', 'ipa',
  // Office macro-bearing documents
  'docm', 'dotm', 'xlsm', 'xltm', 'xlam', 'pptm', 'potm', 'ppam', 'sldm',
]);

// Archive extensions — they commonly carry the above and the user should
// inspect contents before extraction.
const ARCHIVE_EXTENSIONS: ReadonlySet<string> = new Set([
  'zip', '7z', 'rar', 'tar', 'gz', 'tgz', 'bz2', 'tbz2', 'xz', 'txz',
  'cab', 'iso', 'img', 'wim', 'arj', 'lzh', 'lha', 'z',
]);

// MIME types corresponding to the above. Some servers send accurate MIME and
// a sanitized filename, so MIME is a useful second signal.
const EXECUTABLE_MIME_TYPES: ReadonlySet<string> = new Set([
  'application/x-msdownload',
  'application/x-msdos-program',
  'application/x-ms-installer',
  'application/x-msi',
  'application/vnd.microsoft.portable-executable',
  'application/x-executable',
  'application/x-mach-binary',
  'application/x-elf',
  'application/x-sh',
  'application/x-shellscript',
  'application/x-bat',
  'application/x-bash',
  'application/x-csh',
  'application/x-perl',
  'application/x-python',
  'application/x-ruby',
  'application/x-php',
  'application/javascript',
  'application/x-javascript',
  'application/x-vbscript',
  'application/x-powershell',
  'application/java-archive',
  'application/vnd.android.package-archive',
  'application/vnd.apple.installer+xml',
  'application/x-apple-diskimage',
  'application/x-debian-package',
  'application/x-rpm',
  'application/x-iso9660-image',
]);

const ARCHIVE_MIME_TYPES: ReadonlySet<string> = new Set([
  'application/zip',
  'application/x-zip-compressed',
  'application/x-7z-compressed',
  'application/x-rar-compressed',
  'application/vnd.rar',
  'application/x-tar',
  'application/gzip',
  'application/x-gzip',
  'application/x-bzip',
  'application/x-bzip2',
  'application/x-xz',
  'application/x-compress',
  'application/x-compressed',
  'application/x-cab',
  'application/vnd.ms-cab-compressed',
]);

/**
 * Returns the lowercased extension (without dot) of `filename`, or '' when
 * none. Handles double extensions like `.tar.gz` by returning only the last
 * segment — callers wanting `.tar.gz` should string-match the filename.
 */
export function getExtension(filename: string): string {
  const idx = filename.lastIndexOf('.');
  if (idx < 0 || idx === filename.length - 1) return '';
  return filename.slice(idx + 1).toLowerCase().trim();
}

/** True for native executables, installers, and auto-running scripts. */
export function isExecutableAttachment(filename: string, mimeType?: string): boolean {
  const ext = getExtension(filename);
  if (ext && EXECUTABLE_EXTENSIONS.has(ext)) return true;
  if (mimeType && EXECUTABLE_MIME_TYPES.has(mimeType.toLowerCase().split(';')[0].trim())) return true;
  return false;
}

/** True for archive containers (zip/7z/rar/tar/iso/etc.). */
export function isArchiveAttachment(filename: string, mimeType?: string): boolean {
  const ext = getExtension(filename);
  if (ext && ARCHIVE_EXTENSIONS.has(ext)) return true;
  // .tar.gz / .tar.bz2 land on the last segment alone — accept those via the
  // primary check above, no special case needed.
  if (mimeType && ARCHIVE_MIME_TYPES.has(mimeType.toLowerCase().split(';')[0].trim())) return true;
  return false;
}

/**
 * Force Save As (no Open) when an attachment could execute code. Archives
 * count, since they commonly carry executables and we want the user to
 * extract them deliberately.
 */
export function shouldForceSaveAs(filename: string, mimeType?: string): boolean {
  return isExecutableAttachment(filename, mimeType) || isArchiveAttachment(filename, mimeType);
}
