// ESLint rule: files are saved through the platform, not a raw `download` link.
//
// In the mobile app's webview `<a download>` does nothing, so a link or a
// script-built anchor that relies on it saves nothing there. `<DownloadLink>`
// and `saveFile` (src/platform/files.ts) save on both hosts: the browser's
// download on the web, the share sheet in the app. This reports a `download`
// attribute in a template, static or bound, and in a script an assignment to
// `.download` or a `setAttribute('download', ...)`.

const MESSAGE =
  'Save files with <DownloadLink> or saveFile from @/platform: the mobile app ignores a download link.'

export default {
  meta: {
    type: 'problem',
    docs: { description: 'Save files through <DownloadLink> or saveFile, not a download link' },
    schema: [],
    messages: { rawDownload: MESSAGE },
  },
  create(context) {
    const report = (node) => context.report({ node, messageId: 'rawDownload' })

    const scriptVisitor = {
      AssignmentExpression(node) {
        const left = node.left
        if (left.type !== 'MemberExpression') return
        const name = left.computed ? left.property.value : left.property.name
        if (name === 'download') report(node)
      },
      CallExpression(node) {
        const callee = node.callee
        if (
          callee.type === 'MemberExpression' &&
          !callee.computed &&
          callee.property.name === 'setAttribute' &&
          node.arguments[0]?.type === 'Literal' &&
          node.arguments[0].value === 'download'
        ) {
          report(node)
        }
      },
    }

    const services = context.sourceCode.parserServices
    if (!services?.defineTemplateBodyVisitor) return scriptVisitor

    return services.defineTemplateBodyVisitor(
      {
        VAttribute(node) {
          const key = node.key
          const isStatic = !node.directive && key.name === 'download'
          const isBound =
            node.directive &&
            key.name.name === 'bind' &&
            key.argument?.type === 'VIdentifier' &&
            key.argument.name === 'download'
          if (isStatic || isBound) report(node)
        },
      },
      scriptVisitor,
    )
  },
}
