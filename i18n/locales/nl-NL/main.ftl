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

# Persistent shell.
nav-group-work = Werk
nav-group-resources = Bronnen
nav-dashboard = Dashboard
nav-tickets = Tickets
nav-cycles = Cycli
nav-projects = Projecten
nav-devices = Apparaten
nav-assets = Activa
nav-users = Gebruikers
nav-documentation = Documentatie
nav-inbox = Postvak
nav-collapse = Inklappen
nav-search = Zoeken
nav-more = Meer
nav-toggle-sidebar = Zijbalk wisselen
nav-secondary = Secundaire navigatie
user-menu-aria = Gebruikersmenu
user-menu-view-profile = Profiel bekijken
user-menu-account = Account
user-menu-administration = Beheer
user-menu-sign-out = Afmelden
user-menu-guest-name = Gast

# Tickets — lege staten + bulkactiebalk.
ticket-list-empty-no-assigned-message = Geen tickets aan u toegewezen.
ticket-list-empty-showing-all-active = Alle actieve tickets worden weergegeven.
ticket-list-empty-no-match-title = Geen tickets gevonden.
ticket-list-empty-no-match-description = Verwijder filters om meer te zien.
ticket-list-empty-triage-clear-title = Triage afgerond.
ticket-list-empty-triage-clear-description = Nieuwe tickets die nog ingedeeld moeten worden, verschijnen hier.
ticket-list-empty-all-caught-up-title = Alles bijgewerkt.
ticket-list-empty-all-caught-up-description = Geen open tickets aan u toegewezen.
ticket-list-empty-no-active-title = Geen actieve tickets.
ticket-list-empty-no-active-description = Elk ticket is opgelost of geannuleerd.
ticket-list-empty-no-in-view-title = Geen tickets in deze weergave.
ticket-list-empty-no-in-view-description = Pas het filter aan of kies een andere weergave.
ticket-list-bulk-actions-aria = Bulkacties
ticket-list-bulk-status = Status
ticket-list-bulk-priority = Prioriteit
ticket-list-bulk-assign = Toewijzen
ticket-list-bulk-clear-title = Selectie wissen (Esc)
ticket-list-bulk-clear = Wissen
ticket-list-row-density-aria = Rijdichtheid
ticket-list-save-view-title = Huidige status opslaan als privéweergave
ticket-list-recurring-title = Terugkerend ticket
ticket-list-sla-breached-title = SLA overschreden

# Ticketdetails.
ticket-detail-reconnecting-title = Opnieuw verbinden met live updates
ticket-detail-connecting = Verbinden...
ticket-detail-more-actions = Meer acties
ticket-detail-section-details = Ticketdetails
ticket-detail-section-notes = Ticketnotities
ticket-detail-section-comments = Reacties en bijlagen
ticket-detail-prop-title = Titel
ticket-detail-prop-requester = Aanvrager
ticket-detail-prop-assignee = Toegewezen aan
ticket-detail-prop-status = Status
ticket-detail-prop-priority = Prioriteit
ticket-detail-prop-category = Categorie
ticket-detail-prop-created = Aangemaakt
ticket-detail-prop-last-modified = Laatst gewijzigd
ticket-detail-delete-title = Ticket verwijderen
ticket-detail-delete-confirm-heading = Dit ticket verwijderen?
ticket-detail-delete-confirm-body = Dit kan niet ongedaan worden gemaakt. Het ticket en de geschiedenis worden verwijderd.
ticket-detail-delete-cancel = Annuleren
ticket-detail-delete-confirm = Verwijderen

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

# Beheerders-onboarding (eerste start).
onboarding-welcome-title = Welkom bij Nosdesk
onboarding-welcome-subtitle = Laten we beginnen door uw beheerdersaccount aan te maken
onboarding-error-setup-status = Kan de installatiestatus niet verifiëren. Probeer het opnieuw.
onboarding-success-logging-in = Beheerdersaccount aangemaakt. U wordt aangemeld...
onboarding-success-fallback = Account aangemaakt. Log in met uw gegevens.
onboarding-success-fallback-redirect = Account aangemaakt. Log in om door te gaan.
onboarding-error-setup-failed = De installatie is mislukt. Probeer het opnieuw.
onboarding-error-unexpected = Er is een onverwachte fout opgetreden. Probeer het opnieuw.
onboarding-validation-token = Bootstraptoken is vereist
onboarding-validation-name = Beheerdersnaam is vereist
onboarding-validation-email = E-mailadres is vereist
onboarding-validation-email-format = Voer een geldig e-mailadres in
onboarding-validation-password-length = Wachtwoord moet minimaal 8 tekens lang zijn
onboarding-validation-password-mismatch = Wachtwoorden komen niet overeen
onboarding-token-label = Bootstraptoken
onboarding-token-placeholder = Plak het eenmalige token van de server
onboarding-token-hint = Bekijk de opstartlogs van de server voor een installatie-URL, of haal het handmatig op met
onboarding-name-label = Beheerdersnaam
onboarding-name-placeholder = Voer uw volledige naam in
onboarding-email-label = E-mailadres
onboarding-email-placeholder = Voer uw e-mailadres in
onboarding-password-label = Wachtwoord
onboarding-password-placeholder = Kies een sterk wachtwoord (8+ tekens)
onboarding-confirm-password-label = Wachtwoord bevestigen
onboarding-confirm-password-placeholder = Bevestig uw wachtwoord
onboarding-submit = Beheerdersaccount aanmaken
onboarding-submit-loading = Beheerder aanmaken...
onboarding-progress-title = Uw account instellen
onboarding-progress-subtitle = Dit duurt slechts een moment...
onboarding-complete-title = Welkom bij Nosdesk
onboarding-complete-subtitle = Uw beheerdersaccount is klaar.
onboarding-migration-title = Migreren vanaf een andere Nosdesk-instantie?
onboarding-migration-body-prefix = Maak hier een beheerder aan en voer dan
onboarding-migration-body-suffix = uit op de host. De restore vervangt de beheerder door de geïmporteerde gebruikers.
onboarding-security-title = Beveiligingsmelding
onboarding-security-body = Hiermee wordt het eerste beheerdersaccount voor uw Nosdesk-installatie aangemaakt. Kies een sterk wachtwoord; dit account krijgt volledige systeemtoegang.

# MFA-installatiewizard.
mfa-setup-header-default = Voltooi het instellen van uw account
mfa-setup-header-offer = Nog een methode toevoegen?
mfa-setup-header-additional = Reservemethode toevoegen
mfa-setup-subtitle-default = Uw accounttype vereist multifactor-authenticatie voor beveiliging
mfa-setup-subtitle-choose = Kies uw voorkeursmethode voor authenticatie
mfa-setup-subtitle-offer-passkey = Passkeys bieden een snellere, wachtwoordvrije aanmelding
mfa-setup-subtitle-offer-totp = Een authenticator-app biedt een reservemogelijkheid als u uw passkey verliest
mfa-setup-subtitle-passkey-additional = Stel een passkey in voor sneller aanmelden
mfa-setup-subtitle-totp-additional = Stel een authenticator-app in als reserve
mfa-setup-totp-name = Authenticator-app
mfa-setup-totp-description = Gebruik een app als Google Authenticator, Authy of 1Password om tijdgebaseerde codes te genereren
mfa-setup-passkey-name = Passkey
mfa-setup-passkey-description = Gebruik biometrie zoals Face ID, Touch ID of een hardwarebeveiligingssleutel voor wachtwoordvrije aanmelding
mfa-setup-which-title = Welke moet ik kiezen?
mfa-setup-which-passkey-label = Passkeys
mfa-setup-which-passkey-body = zijn veiliger en gemakkelijker, gewoon uw vingerafdruk of gezicht gebruiken.
mfa-setup-which-totp-label = Authenticator-apps
mfa-setup-which-totp-body = werken op elk apparaat en vereisen geen biometrie.
mfa-setup-totp-success-title = Authenticator-app ingesteld!
mfa-setup-totp-success-body = Wilt u ook een passkey toevoegen voor snellere, wachtwoordvrije aanmelding?
mfa-setup-passkey-success-title = Passkey aangemaakt!
mfa-setup-passkey-success-body = Wilt u ook een authenticator-app instellen als reservemethode?
mfa-setup-add-passkey-title = Passkey toevoegen
mfa-setup-add-passkey-description = Gebruik Face ID, Touch ID of een beveiligingssleutel
mfa-setup-add-totp-title = Authenticator-app instellen
mfa-setup-add-totp-description = Gebruik als reserve als u geen toegang meer heeft tot uw passkey
mfa-setup-skip-now = Voor nu overslaan
mfa-setup-back-to-login = Terug naar aanmelden
mfa-setup-back-skip = Overslaan
mfa-setup-back-different = Andere methode kiezen
mfa-setup-error-session-expired = Sessie verlopen. Meld opnieuw aan om MFA in te stellen.
mfa-setup-error-invalid-access = Ongeldige toegang. Doorverwijzen naar aanmelden...

# Wachtwoord herstellen.
password-reset-title = Wachtwoord opnieuw instellen
password-reset-subtitle = Voer hieronder uw nieuwe wachtwoord in
password-reset-success-title = Wachtwoord opnieuw ingesteld!
password-reset-success-body = Uw wachtwoord is bijgewerkt. U kunt nu inloggen met uw nieuwe wachtwoord.
password-reset-success-cta = Naar aanmelden
password-reset-field-new = Nieuw wachtwoord
password-reset-field-new-placeholder = Voer een nieuw wachtwoord in
password-reset-field-confirm = Nieuw wachtwoord bevestigen
password-reset-field-confirm-placeholder = Bevestig het nieuwe wachtwoord
password-reset-req-length = Minimaal 8 tekens
password-reset-match-yes = Wachtwoorden komen overeen
password-reset-match-no = Wachtwoorden komen niet overeen
password-reset-submit = Wachtwoord opnieuw instellen
password-reset-submit-loading = Wachtwoord opnieuw instellen...
password-reset-back-to-login = Terug naar aanmelden
password-reset-error-no-token = Ongeldig of ontbrekend reset-token. Vraag een nieuw wachtwoordherstel aan.
password-reset-error-failed = Wachtwoord herstellen is mislukt. De link is mogelijk verlopen.

# Uitnodiging / gast-ticket accepteren.
accept-invitation-heading-validating = Een moment…
accept-invitation-heading-guest = Bevestig uw ticketinzending
accept-invitation-heading-welcome = Welkom bij { $app }
accept-invitation-subheading-validating = Uw link controleren.
accept-invitation-subheading-guest = Stel een wachtwoord in om uw ticket vrij te geven.
accept-invitation-subheading-invitation = Voltooi het instellen van uw account.
accept-invitation-checking = Uw link controleren…
accept-invitation-invalid-title-guest = Deze bevestigingslink is niet meer geldig
accept-invitation-invalid-title-invitation = Uitnodiging ongeldig
accept-invitation-go-to-signin = Naar aanmelden
accept-invitation-activating-title-guest = Uw ticket vrijgeven…
accept-invitation-activating-title-invitation = Uw account activeren…
accept-invitation-signing-in = Bezig met aanmelden…
accept-invitation-success-title-guest = U bent klaar
accept-invitation-success-title-invitation = Welkom bij { $app }
accept-invitation-manual-login = Meld u aan met het wachtwoord dat u zojuist hebt ingesteld.
accept-invitation-password-label = Wachtwoord
accept-invitation-password-placeholder = Minimaal 8 tekens
accept-invitation-confirm-label = Wachtwoord bevestigen
accept-invitation-confirm-placeholder = Voer het opnieuw in
accept-invitation-req-length = Minimaal 8 tekens
accept-invitation-match-yes = Wachtwoorden komen overeen
accept-invitation-match-no = Wachtwoorden komen niet overeen
accept-invitation-show-password = Wachtwoord tonen
accept-invitation-hide-password = Wachtwoord verbergen
accept-invitation-submit-guest = Bevestigen en ticket vrijgeven
accept-invitation-submit-loading-guest = Bezig met bevestigen…
accept-invitation-submit-invitation = Account activeren
accept-invitation-submit-loading-invitation = Bezig met activeren…
accept-invitation-back-to-signin = Terug naar aanmelden
accept-invitation-error-missing-token = Ongeldige of ontbrekende bevestigingslink.
accept-invitation-error-default = Deze link is ongeldig of verlopen.
accept-invitation-error-validation-failed = Link kon niet worden gevalideerd. Probeer het later opnieuw.
accept-invitation-error-submit = Bevestiging is mislukt. De link is mogelijk verlopen.
