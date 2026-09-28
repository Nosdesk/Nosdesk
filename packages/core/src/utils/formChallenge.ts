// Solve the request form's proof-of-work challenge (see backend
// `utils::form_challenge`): find the number that, appended to the salt, hashes
// to the challenge. Runs in the background while the person types, a fraction
// of a second on a typical device, with no puzzle to answer.

export interface FormChallenge {
  algorithm: 'SHA-256'
  challenge: string
  salt: string
  signature: string
  maxnumber: number
}

export interface FormChallengeSolution {
  challenge: string
  salt: string
  signature: string
  number: number
}

const encoder = new TextEncoder()

async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest('SHA-256', encoder.encode(input))
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('')
}

/** The solution, or null if none exists within `maxnumber` (a bad challenge). */
export async function solveFormChallenge(c: FormChallenge): Promise<FormChallengeSolution | null> {
  // Hash in batches so the page stays responsive between them.
  const batch = 500
  for (let start = 0; start <= c.maxnumber; start += batch) {
    const end = Math.min(start + batch - 1, c.maxnumber)
    const hashes = await Promise.all(
      Array.from({ length: end - start + 1 }, (_, i) => sha256Hex(c.salt + String(start + i))),
    )
    const hit = hashes.indexOf(c.challenge)
    if (hit >= 0) {
      return { challenge: c.challenge, salt: c.salt, signature: c.signature, number: start + hit }
    }
  }
  return null
}
