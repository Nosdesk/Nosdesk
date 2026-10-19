import { afterEach, describe, expect, it } from 'vitest'
import { configureAssetUrl, proxiedAssetResolver } from '@nosdesk/core/transport'
import { useSanitise } from '@/composables/useSanitise'

// Images inside sanitised HTML (comments, articles) go through the platform's
// asset resolver like any other image, so they load in the mobile app.

const mobile = proxiedAssetResolver('nosdesk-asset://localhost')
const mobilePath = (u: string) => u.replace(/^nosdesk-asset:\/\/localhost/, '')

afterEach(() => configureAssetUrl((p) => p, (u) => u))

describe('sanitised images', () => {
  it('get the asset scheme in the mobile app', () => {
    configureAssetUrl(mobile)
    const out = useSanitise().sanitiseHtml('<p><img src="/api/files/tickets/1/shot.png" alt="shot"></p>')
    expect(out).toContain('src="nosdesk-asset://localhost/api/files/tickets/1/shot.png"')
  })

  it('stay as they are on web, and inline images are left alone', () => {
    expect(useSanitise().sanitiseHtml('<img src="/api/files/a.png">')).toContain('src="/api/files/a.png"')
    configureAssetUrl(mobile)
    expect(useSanitise().sanitiseHtml('<img src="data:image/png;base64,AAAA">')).toContain(
      'src="data:image/png;base64,AAAA"',
    )
  })
})

describe('an image the editor already resolved', () => {
  it('is kept in the mobile app, not taken for an off-origin image', () => {
    configureAssetUrl(mobile, mobilePath)
    const out = useSanitise().sanitiseHtml('<img src="nosdesk-asset://localhost/api/files/docs/1/a.png">')
    expect(out).toContain('src="nosdesk-asset://localhost/api/files/docs/1/a.png"')
  })

  it('still drops an off-origin image', () => {
    configureAssetUrl(mobile, mobilePath)
    expect(useSanitise().sanitiseHtml('<img src="https://tracker.example/p.gif">')).not.toContain('src=')
  })
})

describe('editor tables', () => {
  it('keep their column widths, spans and alignment classes', () => {
    const html =
      '<div class="table-scroll"><table><colgroup><col width="160"><col></colgroup><tbody>' +
      '<tr><th class="align-center"><p>Name</p></th><td colspan="2" rowspan="2"><p>VPN</p></td></tr>' +
      '</tbody></table></div>'
    expect(useSanitise().sanitiseHtml(html)).toBe(html)
  })

  it('still drop data attributes and inline styles on cells', () => {
    const out = useSanitise().sanitiseHtml('<table><tbody><tr><td data-colwidth="90" style="color:red">a</td></tr></tbody></table>')
    expect(out).toBe('<table><tbody><tr><td>a</td></tr></tbody></table>')
  })
})
