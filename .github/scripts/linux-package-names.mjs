export const LINUX_PACKAGE_RELEASE = "1";

export function validateLinuxPackageVersion(version) {
  if (typeof version !== "string" || !/^[0-9]+\.[0-9]+\.[0-9]+$/.test(version)) {
    throw new Error(`package version must be stable X.Y.Z, got: ${version}`);
  }
}

export function linuxPackageNames(version) {
  validateLinuxPackageVersion(version);
  return {
    deb: `kesha-voice-kit_${version}-${LINUX_PACKAGE_RELEASE}_amd64.deb`,
    rpm: `kesha-voice-kit-${version}-${LINUX_PACKAGE_RELEASE}.x86_64.rpm`,
  };
}
