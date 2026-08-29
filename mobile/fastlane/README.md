# Mobile release pipeline

Tauri builds the app; Fastlane signs and uploads it. Lanes run from `mobile/`.
CI is `.github/workflows/mobile-release.yml` (manual dispatch: pick `both`,
`ios`, or `android`). The app picks its server at runtime, so one build serves
staging and production.

- **iOS** -> TestFlight (`fastlane ios beta`), signed with a distribution cert
  + App Store profile imported from CI secrets (no `match`).
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
3. **iOS signing, generated once via the API key** (no git repo). With a working
   Ruby + `bundle install` under `mobile/`:
   ```sh
   bundle exec fastlane cert --api_key_path <asc_key.json> --output_path ~/dev/nosdesk-signing --development false
   bundle exec fastlane sigh --api_key_path <asc_key.json> --app_identifier com.nosdesk.app \
     --provisioning_name "Nosdesk App Store" --cert_id <ID> --output_path ~/dev/nosdesk-signing
   ```
   Re-export the `.p12` with a password (`security export`), then base64 the
   `.p12` + `.mobileprovision` into the secrets below. The cert/profile/`.p8`
   live in `~/dev/nosdesk-signing` (back up).
4. **Play service account:** in Google Cloud, a service account with the Play
   Developer API enabled, granted release permissions in the Play Console.
   Download its JSON key.

## GitHub secrets

iOS:
- `APPLE_TEAM_ID`
- `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_CONTENT` (base64 of the App Store
  Connect API `.p8`)
- `IOS_DIST_CERT_P12_BASE64` (base64 of the distribution `.p12`),
  `IOS_DIST_CERT_PASSWORD`, `IOS_PROFILE_BASE64` (base64 of the App Store
  `.mobileprovision`)

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

- the **iOS signing -> Tauri xcodebuild handshake** (the CI runner's Xcode
  version and the export step picking up the imported cert/profile), and
- the **TestFlight / Play uploads** themselves.

Everything else (frontend + Rust + native build, the Android signingConfig, the
Fastfile) is exercised by the local test above and the workflow's build steps.
