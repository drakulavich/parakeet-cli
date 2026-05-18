/// <reference types="@raycast/api">

/* 🚧 🚧 🚧
 * This file is auto-generated from the extension's manifest.
 * Do not modify manually. Instead, update the `package.json` file.
 * 🚧 🚧 🚧 */

/* eslint-disable @typescript-eslint/ban-types */

type ExtensionPreferences = {
  /** `kesha` Binary Path - Absolute path to the `kesha` CLI. Leave blank to auto-detect common global install locations. */
  "keshaBinPath": string,
  /** Max Recording Seconds - Maximum microphone recording duration before the command stops automatically. */
  "maxRecordingSeconds": string
}

/** Preferences accessible in all the extension's commands */
declare type Preferences = ExtensionPreferences

declare namespace Preferences {
  /** Preferences accessible in the `dictate-to-clipboard` command */
  export type DictateToClipboard = ExtensionPreferences & {}
}

declare namespace Arguments {
  /** Arguments passed to the `dictate-to-clipboard` command */
  export type DictateToClipboard = {}
}

