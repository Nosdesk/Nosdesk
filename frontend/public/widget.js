/*
 * Nosdesk help widget loader.
 *
 *   <script src="https://help.example.com/widget.js" async></script>
 *
 * Optional, before the script:
 *   window.NosdeskWidget = {
 *     // Resolve to a visitor token your server signs (see Admin > Help widget).
 *     getToken: async () => (await fetch('/nosdesk-token')).text(),
 *     color: '#0f766e',   // launcher colour
 *     label: 'Help',      // launcher text for screen readers
 *   }
 *
 * Draws a launcher and, on first open, an iframe of this script's URL minus
 * `.js`. Messages go only to that iframe, at its exact origin, and are only
 * accepted from it.
 */
(function () {
  'use strict'
  if (window.__nosdeskWidget) return
  window.__nosdeskWidget = true

  var script = document.currentScript
  if (!script || !script.src) return
  var src = new URL(script.src)
  var widgetOrigin = src.origin
  var widgetUrl = src.origin + src.pathname.replace(/\.js$/, '')
  var cfg = window.NosdeskWidget || {}
  var color = typeof cfg.color === 'string' ? cfg.color : '#111827'
  var label = typeof cfg.label === 'string' ? cfg.label : 'Help'

  var frame = null
  var launcher = document.createElement('button')
  launcher.type = 'button'
  launcher.setAttribute('aria-label', label)
  launcher.setAttribute('aria-expanded', 'false')
  launcher.setAttribute('aria-controls', 'nosdesk-widget-frame')
  launcher.style.cssText =
    'position:fixed;right:20px;bottom:20px;z-index:2147483000;width:56px;height:56px;' +
    'border-radius:28px;border:0;cursor:pointer;box-shadow:0 4px 16px rgba(0,0,0,.2);' +
    'display:flex;align-items:center;justify-content:center;background:' + color + ';color:#fff'
  launcher.innerHTML =
    '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" ' +
    'stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
    '<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>'

  function send(message) {
    if (frame && frame.contentWindow) frame.contentWindow.postMessage(message, widgetOrigin)
  }

  function init() {
    send({ type: 'nosdesk:init', identity: typeof cfg.getToken === 'function' })
  }

  function sendToken() {
    Promise.resolve()
      .then(function () { return cfg.getToken() })
      .then(function (token) { send({ type: 'nosdesk:token', token: typeof token === 'string' ? token : null }) })
      .catch(function () { send({ type: 'nosdesk:token', token: null }) })
  }

  function place() {
    if (!frame) return
    var display = frame.style.display
    var small = window.innerWidth < 480
    frame.style.cssText = small
      ? 'position:fixed;inset:0;width:100%;height:100%;border:0;z-index:2147483001;background:#fff'
      : 'position:fixed;right:20px;bottom:88px;width:380px;height:600px;max-height:calc(100vh - 120px);' +
        'border:0;border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,.24);z-index:2147483001;background:#fff'
    frame.style.display = display
  }

  function open() {
    if (!frame) {
      frame = document.createElement('iframe')
      frame.id = 'nosdesk-widget-frame'
      frame.src = widgetUrl
      frame.title = label
      // It can't navigate this page or open prompts; links open in new tabs.
      frame.setAttribute(
        'sandbox',
        'allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox allow-downloads',
      )
      frame.setAttribute('allow', '')
      frame.setAttribute('referrerpolicy', 'strict-origin-when-cross-origin')
      frame.addEventListener('load', init)
      place()
      document.body.appendChild(frame)
    }
    frame.style.display = 'block'
    launcher.setAttribute('aria-expanded', 'true')
    frame.focus()
  }

  function close() {
    if (frame) frame.style.display = 'none'
    launcher.setAttribute('aria-expanded', 'false')
    launcher.focus()
  }

  launcher.addEventListener('click', function () {
    if (frame && frame.style.display !== 'none') close()
    else open()
  })

  window.addEventListener('message', function (event) {
    if (!frame || event.source !== frame.contentWindow || event.origin !== widgetOrigin) return
    var data = event.data || {}
    if (data.type === 'nosdesk:hello') init()
    else if (data.type === 'nosdesk:close') close()
    else if (data.type === 'nosdesk:token-request' && typeof cfg.getToken === 'function') sendToken()
  })

  window.addEventListener('resize', place)

  window.NosdeskWidget = Object.assign(cfg, { open: open, close: close })

  if (document.body) document.body.appendChild(launcher)
  else document.addEventListener('DOMContentLoaded', function () { document.body.appendChild(launcher) })
})()
