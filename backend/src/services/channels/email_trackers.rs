//! Known-tracker host blocklist for inbound email rendering.
//!
//! Email senders embed 1×1 transparent images that fire on render
//! to confirm the recipient opened the message. From a helpdesk's
//! perspective those pings are useless data exfiltration — the
//! agent's IP, user-agent, and "when they opened the ticket" are
//! none of the sender's business. The image proxy in Pass 3.3
//! prevents the agent's IP from reaching the upstream by routing
//! the fetch through our backend, but the proxy still triggers
//! the tracker if we let it. The cleaner answer is: never fetch
//! at all when the URL is a known tracker.
//!
//! ## Sources
//!
//! Two layers, both bundled as static arrays:
//!
//!   - **DHH's spy-pixel list** ([gist](https://gist.github.com/dhh/360f4dc7ddbce786f8e82b97cdad9d20)).
//!     ~50 named services. Used for the *human attribution*: when
//!     we strip an image and want to tell the agent "stripped a
//!     Mailchimp tracker," this list maps host → display name.
//!   - **Generic 1×1 fallback.** A handful of pattern hosts where
//!     any 1×1 image is presumed to be a pixel. The renderer
//!     can't know the image is 1×1 without fetching it, so for
//!     now this entry is empty — the proxy could detect by size
//!     once it ships in Pass 3.3.
//!
//! [EasyPrivacy](https://easylist.to/easylist/easyprivacy.txt)
//! has a far larger, auto-updating list. Bundling it would let
//! the blocklist self-update weekly via a scheduler task, which
//! the plan documents as the right long-term shape. Out of scope
//! for the first cut; the static list catches the high-volume
//! offenders and we can layer EasyPrivacy on later without
//! changing the call shape here.
//!
//! ## Matching
//!
//! Host-suffix match: an entry of `"example.com"` matches
//! `"example.com"` and `"foo.example.com"` but **not**
//! `"examplebad.com"`. The case is normalised to ASCII lower
//! before comparison. Returns the display name for attribution
//! when matched.

/// (host_suffix, human-readable display name). Suffix match is
/// case-insensitive and rooted at a label boundary so a partial
/// substring match like `track.foo.com` matching
/// `track.foo.com.evil.example` is impossible.
///
/// Names are the operator-recognisable brand for the attribution
/// UI. When two services share a parent company (Mailchimp uses
/// `list-manage.com` for clicks and `mailchimpapp.com` for
/// other tracking), both entries point at the same display name
/// so the UI doesn't surface obscure subsidiary brand names.
static TRACKERS: &[(&str, &str)] = &[
    // -- Marketing / email sending platforms with open-tracking --
    ("mailchimp.com", "Mailchimp"),
    ("list-manage.com", "Mailchimp"),
    ("mailchimpapp.com", "Mailchimp"),
    ("sendgrid.net", "SendGrid"),
    ("sl.sendgrid.net", "SendGrid"),
    ("ss.sendgrid.net", "SendGrid"),
    ("sparkpost.com", "SparkPost"),
    ("constantcontact.com", "Constant Contact"),
    ("constantcontactsites.com", "Constant Contact"),
    ("ctctcdn.com", "Constant Contact"),
    ("getresponse.com", "GetResponse"),
    ("hubspot.com", "HubSpot"),
    ("hubspotemail.net", "HubSpot"),
    ("hs-sites.com", "HubSpot"),
    ("hs-analytics.net", "HubSpot"),
    ("mailgun.org", "Mailgun"),
    ("mailgun.info", "Mailgun"),
    ("postmarkapp.com", "Postmark"),
    ("pstmrk.it", "Postmark"),
    ("amazonses.com", "Amazon SES"),
    ("convertkitmail.com", "ConvertKit"),
    ("ck-assets.com", "ConvertKit"),
    ("activehosted.com", "ActiveCampaign"),
    ("activecampaign.com", "ActiveCampaign"),
    ("klaviyomail.com", "Klaviyo"),
    ("kxcdn.com", "Klaviyo"),
    // -- Per-recipient pixel services --
    ("bananatag.com", "Bananatag"),
    ("mailtrack.io", "Mailtrack"),
    ("yesware.com", "Yesware"),
    ("mxtoolbox.com", "MxToolbox"),
    ("postmastery.com", "Postmastery"),
    ("trackapp.io", "Trackapp"),
    ("hubspotvideo.com", "HubSpot Video"),
    ("docsend.com", "DocSend"),
    // -- Web analytics frequently embedded in marketing emails --
    ("doubleclick.net", "DoubleClick"),
    ("googletagmanager.com", "Google Tag Manager"),
    ("google-analytics.com", "Google Analytics"),
    ("hotjar.com", "Hotjar"),
    ("segment.com", "Segment"),
    ("segment.io", "Segment"),
    // -- Read-receipt browser extensions and consumer trackers --
    ("streak.com", "Streak"),
    ("mxtrack.io", "MxTrack"),
];

/// True when the host matches any tracker entry. Returns the
/// display name for attribution. Case-insensitive; subdomain
/// match is rooted at a label boundary.
pub fn match_tracker(host: &str) -> Option<&'static str> {
    if host.is_empty() {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    for (suffix, name) in TRACKERS {
        if matches_suffix(&lower, suffix) {
            return Some(name);
        }
    }
    None
}

/// True when `host` equals `suffix` exactly, or has it as a
/// label-rooted suffix (i.e. preceded by a `.`). The label-root
/// check is what stops `examplebad.com` matching `example.com`.
fn matches_suffix(host: &str, suffix: &str) -> bool {
    if host == suffix {
        return true;
    }
    if host.len() <= suffix.len() {
        return false;
    }
    let (prefix, tail) = host.split_at(host.len() - suffix.len());
    prefix.ends_with('.') && tail == suffix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_host() {
        assert_eq!(match_tracker("mailchimp.com"), Some("Mailchimp"));
        assert_eq!(match_tracker("sendgrid.net"), Some("SendGrid"));
    }

    #[test]
    fn matches_subdomain() {
        // Marketing platforms typically wrap each customer in a
        // per-tenant subdomain. The match must catch those.
        assert_eq!(match_tracker("track.mailchimp.com"), Some("Mailchimp"));
        assert_eq!(match_tracker("acme.list-manage.com"), Some("Mailchimp"));
        assert_eq!(match_tracker("u123456.ct.sendgrid.net"), Some("SendGrid"));
    }

    #[test]
    fn rejects_partial_label_matches() {
        // The suffix matcher must root at a label boundary so
        // an attacker-controlled `mailchimper.example.com`
        // can't trade on the Mailchimp entry. The matcher only
        // accepts `mailchimp.com` or `*.mailchimp.com`.
        assert_eq!(match_tracker("mailchimper.com"), None);
        assert_eq!(match_tracker("notmailchimp.com"), None);
        assert_eq!(match_tracker("evilmailchimp.com"), None);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(match_tracker("MAILCHIMP.COM"), Some("Mailchimp"));
        assert_eq!(match_tracker("Track.Mailchimp.Com"), Some("Mailchimp"));
    }

    #[test]
    fn unknown_hosts_return_none() {
        assert_eq!(match_tracker("example.com"), None);
        assert_eq!(match_tracker("github.com"), None);
        assert_eq!(match_tracker(""), None);
    }

    #[test]
    fn shared_brand_uses_canonical_name() {
        // Mailchimp owns several domains. All should attribute
        // to "Mailchimp" (the brand the agent recognises) rather
        // than the subsidiary domain name.
        assert_eq!(match_tracker("list-manage.com"), Some("Mailchimp"));
        assert_eq!(match_tracker("mailchimpapp.com"), Some("Mailchimp"));
    }
}
