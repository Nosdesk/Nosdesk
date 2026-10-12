fastlane documentation
----

# Installation

Make sure you have the latest version of the Xcode command line tools installed:

```sh
xcode-select --install
```

For _fastlane_ installation instructions, see [Installing _fastlane_](https://docs.fastlane.tools/#installing-fastlane)

# Available Actions

## iOS

### ios beta

```sh
[bundle exec] fastlane ios beta
```

Build and upload an iOS build to TestFlight

### ios metadata

```sh
[bundle exec] fastlane ios metadata
```

Upload the App Store listing (text, categories, screenshots) without submitting

### ios release

```sh
[bundle exec] fastlane ios release
```

Submit TestFlight build BUILD_NUMBER for App Store review, with the listing

----


## Android

### android beta

```sh
[bundle exec] fastlane android beta
```

Build and upload an Android build to the Play internal track

### android metadata

```sh
[bundle exec] fastlane android metadata
```

Upload the Play listing (text, icon, feature graphic, screenshots) without a release

### android promote

```sh
[bundle exec] fastlane android promote
```

Promote the Play internal track to production

----

This README.md is auto-generated and will be re-generated every time [_fastlane_](https://fastlane.tools) is run.

More information about _fastlane_ can be found on [fastlane.tools](https://fastlane.tools).

The documentation of _fastlane_ can be found on [docs.fastlane.tools](https://docs.fastlane.tools).
