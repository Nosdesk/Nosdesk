# Mobile release pipeline

Tauri builds the app; Fastlane signs and uploads it. Lanes run from `mobile/`.
CI is `.github/workflows/mobile-release.yml` (manual dispatch: pick `both`,
`ios`, or `android`). The app picks its server at runtime, so one build serves
staging and production.

- **iOS** -> TestFlight (`fastlane ios beta`), signed with `match`.
- **Android** -> Play internal track (`fastlane android beta`), signed with an
  upload keystore. `fastlane android promote` moves internal -> production.

Build numbers come from `github.run_number` (env `BUILD_NUMBER` /
`NOSDESK_VERSION_CODE`) so re-uploads never collide.

## One-time account setup (no pipeline removes this)

1. **Apple Developer Program** membership; in **App Store Connect** create the
   app record for bundle id `com.nosdesk.app`. `ITSAppUsesNonExemptEncryption`
   is already set to `false` in `app_iOS/Info.plist`, so TestFlight won't ask
   the export-compliance question.
2. **Google Play Console** ($25 once); create the app for `com.nosdesk.app`,
   enroll in **Play App Signing** with a dedicated *upload* key, and upload the
   first AAB by hand (the API can't create the app). After that `supply` works.
3. **iOS signing (match), once, locally:** create a private git repo for the
   certs, then from `mobile/`:
   ```sh
   MATCH_GIT_URL=<repo> bundle exec fastlane match appstore
   ```
   Give CI read access to that repo (a deploy key, or a PAT via
   `MATCH_GIT_BASIC_AUTHORIZATION`).
4. **Play service account:** in Google Cloud, a service account with the Play
   Developer API enabled, granted release permissions in the Play Console.
   Download its JSON key.

## GitHub secrets

iOS:
- `APPLE_ID`, `APPLE_TEAM_ID`, `ITC_TEAM_ID`
- `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_CONTENT` (base64 of the App Store
  Connect API `.p8`)
- `MATCH_GIT_URL`, `MATCH_PASSWORD`, and either a deploy key or
  `MATCH_GIT_BASIC_AUTHORIZATION` (base64 `user:token`)

Android:
- `ANDROID_KEYSTORE_BASE64` (base64 of the upload `.jks`),
  `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`
- `GOOGLE_SERVICES_JSON_BASE64` (base64 of `app/google-services.json`, which is
  gitignored)
- `PLAY_JSON_KEY_BASE64` (base64 of the Play service-account JSON)

## Local release-signing test (no store credentials needed)

The Android build+sign leg is fully testable offline. Generate a throwaway
keystore, point `keystore.properties` at it, and build:

```sh
cd mobile/src-tauri/gen/android/app
keytool -genkeypair -v -keystore upload-keystore.jks -alias upload \
  -keyalg RSA -keysize 2048 -validity 3650 \
  -storepass test1234 -keypass test1234 -dname "CN=Nosdesk Test"
printf 'storeFile=upload-keystore.jks\nstorePassword=test1234\nkeyAlias=upload\nkeyPassword=test1234\n' > keystore.properties
cd ../../../../..            # back to mobile/
pnpm tauri android build --apk --target aarch64
apksigner verify --print-certs \
  src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.aab 2>/dev/null || \
  "$ANDROID_HOME/build-tools/"*/apksigner verify src-tauri/gen/android/app/build/outputs/apk/universal/release/*.apk
```

Delete `upload-keystore.jks` and `keystore.properties` afterwards (both are
gitignored).

## What can't be verified without credentials

Two legs need real accounts and will likely need one round of iteration:

- the **iOS match -> Tauri xcodebuild signing handshake** (the CI runner's Xcode
  version and the exact match profile name plumbed into
  `update_code_signing_settings`), and
- the **TestFlight / Play uploads** themselves.

Everything else (frontend + Rust + native build, the Android signingConfig, the
Fastfile) is exercised by the local test above and the workflow's build steps.
