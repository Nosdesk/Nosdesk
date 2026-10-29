package com.nosdesk.plugin.share

import androidx.core.content.FileProvider

/**
 * The plugin's own FileProvider class, so its manifest entry doesn't merge
 * with the app's `androidx.core.content.FileProvider` declaration.
 */
class ShareFileProvider : FileProvider()
