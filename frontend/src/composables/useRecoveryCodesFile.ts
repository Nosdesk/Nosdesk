import { useFluent } from 'fluent-vue';

/**
 * Shared recovery-codes `.txt` export. TOTP (MFA) and passkey setup both
 * issue single-use account recovery codes; routing both through here keeps
 * the downloaded file byte-identical (dated filename, numbered list,
 * warning + usage + generated timestamp) so there is one format to reason
 * about rather than one per setup flow.
 */
export function useRecoveryCodesFile() {
  const fluent = useFluent();
  const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

  /** The file body. Exposed so a caller (or a test) can preview it. */
  const buildRecoveryCodesContent = (codes: string[]): string =>
    `${t('recovery-codes-file-title')}

${t('recovery-codes-file-warning')}
${t('recovery-codes-file-usage')}

${t('recovery-codes-file-codes-heading')}
${codes.map((code, index) => `${index + 1}. ${code}`).join('\n')}

${t('recovery-codes-file-generated', { date: new Date().toISOString() })}`;

  /** Trigger a browser download of the codes as a date-stamped `.txt`. */
  const downloadRecoveryCodes = (codes: string[]): void => {
    const blob = new Blob([buildRecoveryCodesContent(codes)], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `nosdesk-recovery-codes-${new Date().toISOString().split('T')[0]}.txt`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
  };

  return { buildRecoveryCodesContent, downloadRecoveryCodes };
}
