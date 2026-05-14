## Fluent-catalogus nl-NL (Nederlands - Nederland).
##
## DRAFT — Eerste vertaling, nog niet door een moedertaalspreker
## nagekeken. Suggesties zijn welkom via PR; veranderingen
## blijven beperkt tot dit bestand, geen codewijzigingen nodig.
##
## Conventie: formele aanspreking ("u") zoals gangbaar in
## Nederlandse zakelijke software.

# Generic
greeting = Hallo { $name }.
unread-count = { $count ->
    [0] Geen nieuwe berichten.
    [one] Eén nieuw bericht.
   *[other] { $count } nieuwe berichten.
}

# Transactional email subjects.
password-reset-subject = Reset uw { $app }-wachtwoord
invitation-subject = U bent uitgenodigd voor { $app } - Account instellen
# Invitation email body.
invitation-title = Welkom bij { $app }!
invitation-greeting = Hallo <strong>{ $name }</strong>,
invitation-intro = U bent uitgenodigd voor <strong>{ $app }</strong> door <strong>{ $by }</strong>.
invitation-action-prompt = Klik op de knop hieronder om uw account in te stellen en een wachtwoord aan te maken:
invitation-cta-label = Account instellen
invitation-notice-expiry = Deze uitnodigingslink verloopt over <strong>7 dagen</strong>
invitation-notice-create-password = U maakt tijdens het instellen een wachtwoord aan
invitation-notice-strong-password = Kies een sterk wachtwoord van minstens 8 tekens
invitation-notice-unexpected = Als u deze uitnodiging niet verwachtte, kunt u deze e-mail negeren
invitation-footer = Neem voor vragen contact op met uw systeembeheerder.
invitation-body-text =
    Hallo { $name },

    U bent uitgenodigd voor { $app } door { $by }.

    Open deze link in uw browser om uw account in te stellen en een wachtwoord aan te maken:

    { $link }

    Een paar punten om te weten:
      - Deze uitnodiging verloopt over 7 dagen.
      - U maakt tijdens het instellen een wachtwoord aan.
      - Kies een sterk wachtwoord van minstens 8 tekens.
      - Als u deze uitnodiging niet verwachtte, kunt u deze e-mail negeren.

    -- { $app }

# Notification email subjects.
notif-ticket-assigned = [{ $app }] Ticket toegewezen: { $title }
notif-ticket-status-changed = [{ $app }] Status gewijzigd: { $title }
notif-comment-added = [{ $app }] Nieuwe reactie: { $title }
notif-mentioned = [{ $app }] { $actor } heeft u genoemd
notif-ticket-created-requester = [{ $app }] Ticket aangemaakt: { $title }
notif-doc-page-updated = [{ $app }] Pagina bijgewerkt: { $title }
# Notification email body.
notif-body-fallback = U hebt een nieuwe melding.
notif-from-row = <strong>Van:</strong> { $actor }
notif-cta-view-in = Openen in { $app }
notif-footer-preferences = U ontvangt deze e-mail vanwege uw meldingsvoorkeuren.
notif-body-text =
    { $title }

    { $body }

    Van: { $actor }

    Openen in { $app }: { $cta }

    -- U ontvangt deze e-mail vanwege uw meldingsvoorkeuren in { $app }.

# Login + MFA challenge view.
login-subtitle = Log in op uw account
login-email-label = E-mail
login-email-placeholder = Voer uw e-mailadres in
login-password-label = Wachtwoord
login-password-placeholder = Voer uw wachtwoord in
login-password-show = Wachtwoord tonen
login-password-hide = Wachtwoord verbergen
login-forgot-password = Wachtwoord vergeten?
login-submit = Inloggen
login-submitting = Bezig met inloggen...
login-passkey-cta = Inloggen met passkey
login-passkey-authenticating = Authenticatie...
login-microsoft-cta = Inloggen met Microsoft Entra
login-microsoft-connecting = Verbinden...
login-microsoft-logout-title = Uitloggen bij Microsoft-account
login-oidc-cta = Inloggen met { $provider }
login-oidc-logout-title = Uitloggen bij { $provider }-account
login-oidc-connecting = Verbinden...
login-divider-or = of
login-mfa-title = Tweefactorauthenticatie
login-mfa-subtitle = Voer uw authenticatiecode in
login-mfa-code-label = Authenticatiecode
login-mfa-code-help = Voer de zescijferige code uit uw authenticator-app in, of een 8-karakter back-upcode
login-mfa-back = Terug
login-mfa-verify = Verifiëren en inloggen
login-mfa-verifying = Verifiëren...
login-passkey-mfa-verified = Wachtwoord geverifieerd voor { $email }
login-passkey-mfa-verify-cta = Verifiëren met passkey
login-passkey-mfa-use-recovery = Een herstelcode gebruiken
login-passkey-mfa-back-to-login = Terug naar inloggen
login-recovery-code-label = Herstelcode
login-recovery-code-placeholder = Voer de herstelcode in
login-recovery-code-help = Voer een van de 8-karakter herstelcodes in die u tijdens de configuratie hebt opgeslagen

# Forgot-password modal.
forgot-password-title = Wachtwoord opnieuw instellen
forgot-password-close-modal = Venster sluiten
forgot-password-intro = Voer uw e-mailadres in en we sturen u een link om uw wachtwoord opnieuw in te stellen.
forgot-password-email-label = E-mailadres
forgot-password-email-placeholder = u@voorbeeld.nl
forgot-password-cancel = Annuleren
forgot-password-submit = Resetlink versturen
forgot-password-submitting = Versturen...
forgot-password-error-default = Resetmail kon niet worden verzonden. Probeer het opnieuw.
forgot-password-success-title = Controleer uw e-mail
forgot-password-success-body = Als er een account bestaat met dit e-mailadres, hebben we een wachtwoord-resetlink naar { $email } gestuurd
forgot-password-success-important = Belangrijk:
forgot-password-success-tip-expiry = De link verloopt over <strong>1 uur</strong>
forgot-password-success-tip-spam = Controleer uw spam-map als u hem niet ziet
forgot-password-success-tip-close = U kunt dit venster nu sluiten
forgot-password-success-done = Klaar

# Profile settings tabs.
settings-tab-profile = Profiel
settings-tab-appearance = Weergave
settings-tab-language = Taal
settings-tab-notifications = Meldingen
settings-tab-security = Beveiliging
settings-sidebar-heading = Instellingen
settings-subtitle = Beheer uw profiel, voorkeuren en beveiligingsinstellingen
settings-loading-user = Gebruikersinstellingen laden...
settings-user-heading = Gebruikersinstellingen
settings-section-suffix = - Instellingen

# Dashboard.
dashboard-greeting-morning = Goedemorgen { $name }.
dashboard-greeting-afternoon = Goedemiddag { $name }.
dashboard-greeting-evening = Goedenavond { $name }.
dashboard-greeting-late-night = Hallo { $name }, het wordt laat.
dashboard-subtitle = Welkom op uw { $app }-dashboard
dashboard-edit-button = Dashboard bewerken
dashboard-guest-fallback = Gast

# Lege staten voor de belangrijkste overzichten.
empty-documentation-grid-title = Nog geen documentatie
empty-documentation-grid-description = Maak uw eerste documentatiepagina om te beginnen.
empty-documentation-index-title = Start uw kennisbank
empty-documentation-index-description = Documentatiepagina's leggen procedures, FAQ's en beleid van uw team vast. Maak de eerste pagina om te beginnen.
empty-documentation-archived-title = Geen gearchiveerde pagina's
empty-documentation-archived-description = Gearchiveerde pagina's verschijnen hier.
empty-documentation-trash-title = Prullenbak is leeg
empty-documentation-trash-description = Verwijderde pagina's verschijnen hier.
empty-project-search-title = Geen projecten gevonden
empty-project-search-description = Probeer uw zoekopdracht aan te passen
empty-project-available-title = Geen projecten beschikbaar
empty-project-available-description = Maak een project om te beginnen
empty-device-search-prompt-title = Zoek naar apparaten
empty-device-search-prompt-description = Begin met typen om apparaten te vinden op naam, serienummer of gebruiker
empty-device-search-title = Geen apparaten gevonden
empty-device-search-description = Probeer uw zoekopdracht aan te passen
empty-users-default-title = Geen gebruikers gevonden
empty-users-default-description = Nodig gebruikers uit om te beginnen
empty-users-search-title = Geen gebruikers gevonden
empty-users-search-description = Probeer uw zoekopdracht aan te passen
empty-devices-default-title = Geen apparaten gevonden
empty-devices-default-description = Voeg uw eerste apparaat toe om te beginnen
empty-devices-search-title = Geen apparaten gevonden
empty-devices-search-description = Probeer uw zoekopdracht of filters aan te passen
empty-groups-title = Nog geen groepen
empty-groups-description = Maak uw eerste groep om gebruikers te organiseren
empty-assignment-rules-title = Nog geen toewijzingsregels
empty-assignment-rules-description = Maak uw eerste regel om tickets automatisch toe te wijzen
empty-webhooks-title = Geen webhooks
empty-webhooks-description = Maak een webhook om gebeurtenissen naar externe diensten te sturen
empty-api-tokens-title = Geen API-tokens
empty-api-tokens-description = Maak een API-token om programmatische toegang tot de API mogelijk te maken
empty-categories-title = Nog geen categorieën
empty-categories-description = Maak categorieën om tickets te organiseren
empty-plugins-installed-title = Geen plugins geïnstalleerd
empty-plugins-installed-description = Plugins breiden { $app } uit met aangepaste integraties en functies. Blader door het register voor één-klik-installaties.

# Settings.
settings-localization-title = Taal en tijdzone
settings-localization-help = Bepaalt de taal van berichten en hoe datums worden weergegeven. De siteinstelling wordt gebruikt als u niets selecteert.
settings-language-label = Taal
settings-timezone-label = Tijdzone
settings-locale-site-default = Sitestandaard
settings-locale-en-US = Engels (Verenigde Staten)
settings-locale-en-GB = Engels (Verenigd Koninkrijk)
settings-locale-en-AU = Engels (Australië)
settings-locale-fr-FR = Frans (Frankrijk)
settings-locale-nl-NL = Nederlands (Nederland)
settings-timezone-browser-detected = Browser-detectie ({ $tz })
settings-timezone-use-device = Apparaat-tijdzone gebruiken
settings-timezone-search-placeholder = Zoek op stad of UTC-offset (bijv. Amsterdam, UTC+1)
settings-timezone-no-matches = Geen tijdzones gevonden
settings-save = Opslaan
settings-saving = Opslaan...
settings-localization-saved = Taal- en tijdzonevoorkeuren opgeslagen
settings-localization-save-failed = Opslaan van voorkeuren mislukt

# Channel auto-acknowledgement.
auto-ack-default-template = Uw verzoek (#{ $ticket_id }) is ontvangen en wordt beoordeeld door ons supportteam. Antwoord op deze e-mail om aanvullende opmerkingen toe te voegen.

# Inbox-time connecting copy.
inbox-time-just-now = Zojuist
inbox-time-yesterday = Gisteren om { $time }
inbox-time-weekday = { $day } om { $time }

# Password-reset email body.
password-reset-title = Wachtwoord-resetverzoek
password-reset-greeting = Hallo <strong>{ $name }</strong>,
password-reset-intro = We hebben een verzoek ontvangen om het wachtwoord van uw <strong>{ $app }</strong>-account opnieuw in te stellen. Als u dit verzoek niet hebt ingediend, kunt u deze e-mail negeren.
password-reset-action-prompt = Klik op de knop hieronder om uw wachtwoord opnieuw in te stellen:
password-reset-cta-label = Wachtwoord resetten
password-reset-notice-expiry = Deze link verloopt over <strong>1 uur</strong>
password-reset-notice-single-use = Deze link kan slechts <strong>één keer</strong> worden gebruikt
password-reset-notice-never-share = Deel deze link nooit met iemand
password-reset-notice-account-security = Als u dit reset-verzoek niet hebt gedaan, beveilig dan onmiddellijk uw account
password-reset-footer = Neem voor vragen contact op met uw systeembeheerder.
password-reset-body-text =
    Hallo { $name },

    We hebben een verzoek ontvangen om het wachtwoord van uw { $app }-account opnieuw in te stellen. Als u dit verzoek niet hebt ingediend, kunt u deze e-mail negeren.

    Open deze link in uw browser om uw wachtwoord opnieuw in te stellen:

    { $link }

    Beveiligingsopmerkingen:
      - Deze link verloopt over 1 uur.
      - Deze link kan slechts één keer worden gebruikt.
      - Deel deze link nooit met iemand.
      - Als u dit verzoek niet hebt gedaan, beveilig dan uw account.

    Neem voor vragen contact op met uw systeembeheerder.

    -- { $app }
