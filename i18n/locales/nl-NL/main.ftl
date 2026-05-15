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

# Beheer: auditlogboek.
admin-audit-title = Auditlogboek
admin-audit-description = Forensisch overzicht van wie wat heeft gewijzigd op gecontroleerde entiteiten. Standaard de laatste 7 dagen en de 50 nieuwste vermeldingen; verfijn met de onderstaande filters.
admin-audit-filter-entity = Entiteit
admin-audit-filter-any = Alle
admin-audit-filter-entity-id = Entiteit-ID
admin-audit-filter-entity-id-placeholder = bijv. 42
admin-audit-filter-actor = Actor-UUID
admin-audit-filter-actor-placeholder = bijv. 0192…
admin-audit-clear-filters = Filters wissen
admin-audit-empty-title = Geen auditvermeldingen
admin-audit-empty-description = Er zijn geen gecontroleerde entiteiten gewijzigd in het geselecteerde venster, of de filters sluiten alle rijen uit.
admin-audit-by = door
admin-audit-corr = corr
admin-audit-diff-field = Veld
admin-audit-diff-old = Oud
admin-audit-diff-new = Nieuw
admin-audit-no-diff = Geen veldniveau-diff voor deze vermelding.
admin-audit-op-created = Aangemaakt
admin-audit-op-updated = Bijgewerkt
admin-audit-op-deleted = Verwijderd
admin-audit-actor-system = systeem
admin-audit-load-more = Meer laden
admin-audit-loading-more = Laden…
admin-audit-error-load = Kon auditlogboek niet laden
admin-audit-error-load-more = Kon meer auditvermeldingen niet laden

# Beheer: e-mailsuppressielijst.
admin-suppressions-title = E-mailsuppressielijst
admin-suppressions-description = Adressen waar we niet naartoe proberen te bezorgen. Harde bounces (5xx SMTP / 5.x.x enhanced status) komen hier automatisch terecht; voeg handmatig toe voor naleving of klachten. Zachte bounces (4xx, tijdelijk) worden nooit automatisch onderdrukt.
admin-suppressions-count-singular = suppressie
admin-suppressions-count-plural = suppressies
admin-suppressions-add-title = Suppressie toevoegen
admin-suppressions-add-email-placeholder = gebruiker@voorbeeld.com
admin-suppressions-add-note-placeholder = Optionele notitie (nalevingsverzoek, enz.)
admin-suppressions-adding = Toevoegen…
admin-suppressions-add = Toevoegen
admin-suppressions-empty-title = Geen suppressies
admin-suppressions-empty-description = Hard-bounced ontvangers en handmatig toegevoegde adressen verschijnen hier.
admin-suppressions-bounce-count-title = { $count } keer gebounced
admin-suppressions-remove = Verwijderen
admin-suppressions-confirm-title = Verwijderen van suppressielijst?
admin-suppressions-confirm-message = Toekomstige verzendingen naar dit adres worden normaal geprobeerd. Als de oorspronkelijke fout een harde bounce was, mislukken ze waarschijnlijk en worden ze opnieuw onderdrukt.
admin-suppressions-confirm-keep = Onderdrukt houden
admin-suppressions-load-more = Meer laden
admin-suppressions-loading-more = Laden…
admin-suppressions-error-load = Kon suppressies niet laden
admin-suppressions-error-load-more = Kon meer niet laden
admin-suppressions-error-add = Kon suppressie niet toevoegen
admin-suppressions-error-remove = Kon niet verwijderen
admin-suppressions-reason-hard-bounce = harde bounce
admin-suppressions-reason-manual = handmatig

# Beheer: uitgaande-e-mailwachtrij.
admin-email-queue-title = Uitgaande-e-mailwachtrij
admin-email-queue-description = Duurzame registratie van elke verzendpoging. De worker leegt openstaande rijen elke paar seconden; mislukte verzendingen worden opnieuw geprobeerd met exponentiële backoff. Gebruik deze weergave om te onderzoeken waarom een melding niet is verstuurd.
admin-email-queue-stat-pending = In behandeling
admin-email-queue-stat-oldest = Oudste: { $age }
admin-email-queue-stat-sent = Verzonden
admin-email-queue-stat-failed = Mislukt (opnieuw proberen)
admin-email-queue-stat-dead = Dood (geen herhaling)
admin-email-queue-filter-status = Status
admin-email-queue-filter-ticket = Ticket-ID
admin-email-queue-filter-ticket-placeholder = 42
admin-email-queue-filter-domain = Domein van ontvanger
admin-email-queue-filter-domain-placeholder = voorbeeld.com
admin-email-queue-clear-filters = Filters wissen
admin-email-queue-status-pending = in behandeling
admin-email-queue-status-sending = verzenden
admin-email-queue-status-sent = verzonden
admin-email-queue-status-failed = mislukt
admin-email-queue-status-dead = dood
admin-email-queue-status-suppressed = onderdrukt
admin-email-queue-empty-title = Geen uitgaande e-mails
admin-email-queue-empty-description = Er zijn recent geen antwoorden verstuurd, of de filters sluiten alle rijen uit.
admin-email-queue-bounced = Bounce
admin-email-queue-bounced-with-diagnostic = Bounce: { $diagnostic }
admin-email-queue-bounced-no-diagnostic = Bounce (geen upstream-diagnose vastgelegd)
admin-email-queue-attempts-title = { $count } poging(en)
admin-email-queue-retry-now = Nu opnieuw proberen
admin-email-queue-cancel = Annuleren
admin-email-queue-details = Details
admin-email-queue-hide = Verbergen
admin-email-queue-field-recipient = Ontvanger
admin-email-queue-field-channel = Kanaal
admin-email-queue-field-ticket = Ticket
admin-email-queue-field-comment = Reactie
admin-email-queue-field-next-attempt = Volgende poging
admin-email-queue-field-sent-at = Verzonden om
admin-email-queue-field-failed-at = Mislukt om
admin-email-queue-field-smtp-code = SMTP-code
admin-email-queue-field-last-error = Laatste fout
admin-email-queue-field-bounced-at = Gebounced om
admin-email-queue-field-bounce-recipient = Bounce-ontvanger
admin-email-queue-field-bounce-reason = Reden van bounce
admin-email-queue-load-more = Meer laden
admin-email-queue-loading-more = Laden…
admin-email-queue-confirm-title = E-mail in wachtrij annuleren?
admin-email-queue-confirm-message = De e-mail wordt als onderdrukt gemarkeerd en niet verzonden.
admin-email-queue-confirm-yes = Verzending annuleren
admin-email-queue-confirm-no = Behouden
admin-email-queue-error-load = Kon e-mailwachtrij niet laden
admin-email-queue-error-load-more = Kon meer wachtrij-items niet laden
admin-email-queue-error-stats = Kon statistieken niet laden
admin-email-queue-error-retry = Opnieuw proberen mislukt
admin-email-queue-error-cancel = Annuleren mislukt

# Beheer: workflowstatussen.
admin-workflow-states-title = Workflow
admin-workflow-states-description = Voeg ticketstatussen toe binnen de standaard workflowcategorieën. Categorieën liggen vast, zodat SLA, dashboards en automatisering consistent blijven werken tussen teams. Nieuwe tickets komen in de status die als standaard is gemarkeerd.
admin-workflow-states-count-singular = status
admin-workflow-states-count-plural = statussen
admin-workflow-states-default-badge = Standaard
admin-workflow-states-make-default = Standaard maken
admin-workflow-states-archive-title = Status archiveren
admin-workflow-states-archive-disabled-title = Kan standaardstatus niet archiveren
admin-workflow-states-archive-confirm = "{ $name }" archiveren? Bestaande tickets behouden deze status.
admin-workflow-states-empty-category = Geen statussen in deze categorie.
admin-workflow-states-add-placeholder = Statusnaam toevoegen
admin-workflow-states-add = Toevoegen
admin-workflow-states-error-name-required = Naam is vereist
admin-workflow-states-error-load = Kon workflowstatussen niet laden
admin-workflow-states-error-save = Kon status niet opslaan
admin-workflow-states-error-default = Kon standaard niet instellen
admin-workflow-states-error-archive = Kon status niet archiveren
admin-workflow-states-error-promote-first = Promoveer eerst een andere status tot standaard voordat u deze archiveert.
admin-workflow-states-error-create = Kon status niet aanmaken
admin-workflow-states-saved = Opgeslagen
admin-workflow-states-default-flash = { $name } is nu de standaardstatus voor nieuwe tickets
admin-workflow-states-archived-flash = { $name } gearchiveerd
admin-workflow-states-added-flash = { $name } toegevoegd aan { $category }

# Beheer-chrome.
admin-back-to-dashboard = Terug naar dashboard
admin-heading = Beheer
admin-search-placeholder = Zoeken in instellingen...
admin-search-empty = Geen instellingen komen overeen met "{ $query }"
admin-clear-search = Zoekopdracht wissen
admin-index-subtitle = Beheer uw systeeminstellingen, integraties en werkruimteconfiguratie

admin-nav-group-tickets = Tickets en workflow
admin-nav-group-integrations = Integraties
admin-nav-group-compliance = Naleving
admin-nav-group-appearance = Uiterlijk en meldingen
admin-nav-group-system = Systeem

admin-nav-groups-title = Groepen
admin-nav-groups-description = Beheer gebruikersgroepen en lidmaatschappen
admin-nav-categories-title = Categorieën
admin-nav-categories-description = Ticketcategorieën en zichtbaarheid per groep configureren
admin-nav-assignment-rules-title = Toewijzingsregels
admin-nav-assignment-rules-description = Configureer automatische tickettoewijzing op basis van regels
admin-nav-workflow-title = Workflow
admin-nav-workflow-description = Voeg ticketstatussen toe binnen de standaard workflowcategorieën
admin-nav-sla-title = SLA
admin-nav-sla-description = Servicelevel-beleid en kalenders voor werktijden
admin-nav-api-tokens-title = API-tokens
admin-nav-api-tokens-description = Beheer API-tokens voor programmatische toegang
admin-nav-webhooks-title = Webhooks
admin-nav-webhooks-description = Configureer webhooks om gebeurtenissen naar externe services te sturen
admin-nav-plugins-title = Plug-ins
admin-nav-plugins-description = Beheer geïnstalleerde plug-ins en integraties
admin-nav-data-import-title = Gegevensimport
admin-nav-data-import-description = Importeer gegevens uit Intune, CSV-bestanden en andere bronnen
admin-nav-channels-email-title = E-mailinname
admin-nav-channels-email-description = Pol een support-mailbox via IMAP en zet berichten om in tickets
admin-nav-email-queue-title = E-mailwachtrij
admin-nav-email-queue-description = Duurzame wachtrij voor uitgaande e-mail: status, herhalingen, bounces en acties per rij
admin-nav-email-suppressions-title = E-mailsuppressies
admin-nav-email-suppressions-description = Adressen die geblokkeerd zijn voor uitgaande verzending, automatisch gevuld door harde bounces
admin-nav-audit-log-title = Auditlogboek
admin-nav-audit-log-description = Forensisch overzicht van wijzigingen, gevoed door triggers per tabel
admin-nav-branding-title = Branding
admin-nav-branding-description = Pas het uiterlijk en de branding van de applicatie aan
admin-nav-email-settings-title = E-mailconfiguratie
admin-nav-email-settings-description = Configureer SMTP-instellingen en verstuur test-e-mails
admin-nav-guest-access-title = Gasttoegang
admin-nav-guest-access-description = Bepaal wat niet-geauthenticeerde bezoekers kunnen zien en indienen
admin-nav-auth-providers-title = Authenticatieproviders
admin-nav-auth-providers-description = Configureer SSO, Microsoft Entra en lokale authenticatie
admin-nav-search-title = Zoeken
admin-nav-search-description = Beheer de zoekindex en bekijk indexeringsstatistieken
admin-nav-system-settings-title = Systeeminstellingen
admin-nav-system-settings-description = Beheer opslag, ruim oude bestanden op en systeemonderhoud
admin-nav-backup-restore-title = Back-up en herstel
admin-nav-backup-restore-description = Exporteer en herstel systeemgegevens en bijlagen

# Beheer: systeeminstellingen.
admin-system-title = Systeeminstellingen
admin-system-storage-title = Opslagbeheer
admin-system-storage-description = Verwijder oude profielfoto's en avatars die niet meer nodig zijn om schijfruimte vrij te maken.
admin-system-storage-clean = Opschonen
admin-system-storage-cleaning = Opschonen...
admin-system-storage-confirm-title = Verouderde afbeeldingen opschonen?
admin-system-storage-confirm-message = Deze actie kan niet ongedaan worden gemaakt.
admin-system-storage-confirm-label = Opschonen
admin-system-cleanup-success = Opschonen voltooid
admin-system-cleanup-failed = Opschonen mislukt
admin-system-cleanup-stat-avatars = Avatars:
admin-system-cleanup-stat-banners = Banners:
admin-system-cleanup-stat-thumbnails = Miniaturen:
admin-system-cleanup-stat-checked = Gecontroleerd:
admin-system-cleanup-stat-errors = Fouten:
admin-system-cleanup-view-errors = Fouten bekijken ({ $count })
admin-system-cleanup-error-unexpected = Er is een onverwachte fout opgetreden tijdens het opschonen van afbeeldingen

# Beheer: zoekindexbeheer.
admin-search-mgmt-title = Beheer zoekindex
admin-search-mgmt-description = Beheer de full-text zoekindex voor tickets, documentatie, apparaten en gebruikers.
admin-search-mgmt-stats-title = Indexstatistieken
admin-search-mgmt-refresh = Vernieuwen
admin-search-mgmt-total-documents = Totaal documenten
admin-search-mgmt-index-size = Indexgrootte
admin-search-mgmt-status = Status
admin-search-mgmt-status-rebuilding = Opnieuw opbouwen
admin-search-mgmt-status-ready = Gereed
admin-search-mgmt-entity-types = Entiteitstypen
admin-search-mgmt-stats-error = Kon zoekindexstatistieken niet ophalen
admin-search-mgmt-rebuild-title = Zoekindex opnieuw opbouwen
admin-search-mgmt-rebuild-description = Bouwt de volledige zoekindex opnieuw op vanuit de database. Indexeert alle tickets, reacties, documentatiepagina's, bijlagen, apparaten en gebruikers opnieuw. Gebruik dit als zoekresultaten ontbreken of verouderd zijn.
admin-search-mgmt-rebuild = Index opnieuw opbouwen
admin-search-mgmt-rebuilding = Opnieuw opbouwen...
admin-search-mgmt-rebuild-success = Index succesvol opnieuw opgebouwd
admin-search-mgmt-rebuild-failed = Opnieuw opbouwen mislukt
admin-search-mgmt-rebuild-stat-tickets = Tickets:
admin-search-mgmt-rebuild-stat-comments = Reacties:
admin-search-mgmt-rebuild-stat-docs = Documenten:
admin-search-mgmt-rebuild-stat-attachments = Bijlagen:
admin-search-mgmt-rebuild-stat-devices = Apparaten:
admin-search-mgmt-rebuild-stat-users = Gebruikers:
admin-search-mgmt-rebuild-stat-total = Totaal:
admin-search-mgmt-rebuild-confirm-title = Zoekindex opnieuw opbouwen?
admin-search-mgmt-rebuild-confirm-message = Dit kan even duren, afhankelijk van de hoeveelheid data.
admin-search-mgmt-rebuild-confirm-label = Opnieuw opbouwen
admin-search-mgmt-rebuild-error-unexpected = Er is een onverwachte fout opgetreden tijdens het opnieuw opbouwen van de index

# Beheer: E-mailconfiguratie.
admin-email-settings-title = E-mailconfiguratie
admin-email-settings-description = Bekijk de status van de e-mailconfiguratie en verstuur test-e-mails. E-mailinstellingen worden geconfigureerd via omgevingsvariabelen.
admin-email-settings-env-notice-prefix = E-mailinstellingen worden geconfigureerd via omgevingsvariabelen in uw
admin-email-settings-env-notice-suffix = bestand of Docker-omgeving. Gebruik "Test-e-mail versturen" om te controleren of de configuratie werkt.
admin-email-settings-loading = E-mailconfiguratie laden...
admin-email-settings-service = SMTP-e-mailservice
admin-email-settings-configured = Geconfigureerd
admin-email-settings-not-configured = Niet geconfigureerd
admin-email-settings-enabled = Ingeschakeld
admin-email-settings-server = Server
admin-email-settings-username = Gebruikersnaam
admin-email-settings-from-address = Afzender
admin-email-settings-password = Wachtwoord
admin-email-settings-password-not-set = Niet ingesteld
admin-email-settings-env-vars-label = Env:
admin-email-settings-test-send = Test verzenden:
admin-email-settings-test-placeholder = ontvanger@voorbeeld.com
admin-email-settings-test-send-button = Verzenden
admin-email-settings-test-sending = Verzenden...
admin-email-settings-empty-title = E-mail is niet geconfigureerd
admin-email-settings-empty-description = Configureer e-mailinstellingen in uw omgevingsvariabelen om e-mailfunctionaliteit in te schakelen
admin-email-settings-error-load = Kon e-mailconfiguratie niet laden
admin-email-settings-error-no-address = Voer een e-mailadres in
admin-email-settings-error-bad-address = Voer een geldig e-mailadres in
admin-email-settings-test-success = Test-e-mail verzonden
admin-email-settings-error-test = Kon test-e-mail niet verzenden

# Beheer: Gasttoegang.
admin-guest-title = Gasttoegang
admin-guest-description = Bepaal wat niet-geauthenticeerde bezoekers kunnen zien en indienen. Alles staat standaard uit.
admin-guest-loading = Gasttoegangsinstellingen laden...
admin-guest-features-title = Openbare functies
admin-guest-toggle-tickets-label = Gasttickets accepteren
admin-guest-toggle-tickets-description = Toont een openbaar ticketformulier op /submit-ticket.
admin-guest-toggle-lookup-label = Ticketstatus opzoeken voor gasten
admin-guest-toggle-lookup-description = Laat gasten de status controleren via een privélink die bij indiening wordt teruggegeven.
admin-guest-toggle-public-docs-label = Openbare documentatie
admin-guest-toggle-public-docs-description = Toont pagina's gemarkeerd als 'public' op /docs zonder dat inloggen vereist is.
admin-guest-toggle-kb-search-label = Openbare kennisbankzoekfunctie
admin-guest-toggle-kb-search-description = Zoeken in openbare documentatie. Vereist 'Openbare documentatie' ingeschakeld.
admin-guest-toggle-help-label = Zelfhulppagina
admin-guest-toggle-help-description = Statische /help-pagina met links naar wachtwoordherstel en ticketindiening.
admin-guest-submissions-title = Gastticketindieningen
admin-guest-submissions-description = Gedrag voor tickets die via het openbare formulier worden ingediend.
admin-guest-toggle-email-verification-label = E-mailbevestiging vereisen
admin-guest-toggle-email-verification-description = Houdt indieningen vast totdat de aanvrager bevestigt via e-mail. Geeft hen ook toegang tot het portaal.
admin-guest-toggle-attachments-label = Bijlagen toestaan
admin-guest-toggle-attachments-description = Indieners kunnen afbeeldingen, PDF's en tekst/log-bestanden toevoegen (≤10 MB elk, max 5 per ticket).
admin-guest-default-priority-label = Standaardprioriteit
admin-guest-default-priority-hint = Wordt toegepast op elke gastindiening. Technici kunnen achteraf opnieuw triëren.
admin-guest-priority-low = Laag
admin-guest-priority-medium = Gemiddeld
admin-guest-priority-high = Hoog
admin-guest-intro-message-label = Introductiebericht
admin-guest-intro-message-optional = (optioneel)
admin-guest-intro-message-placeholder = bv. Voor dringende storingen bel 555-1234. Bekijk eerst /docs.
admin-guest-intro-message-hint = Wordt boven het openbare formulier getoond. Platte tekst, regeleinden blijven behouden.
admin-guest-intro-message-count = { $count } / 500
admin-guest-rate-limit-label = Snelheidslimiet
admin-guest-rate-limit-suffix = per IP / uur
admin-guest-rate-limit-hint = Verlaag dit als u spam ziet vanaf gedeelde IP's.
admin-guest-unsaved = Niet-opgeslagen wijzigingen
admin-guest-save = Instellingen opslaan
admin-guest-saving = Opslaan...
admin-guest-error-load = Kon gastinstellingen niet laden
admin-guest-error-save = Kon gastinstellingen niet opslaan
admin-guest-saved = Gasttoegangsinstellingen opgeslagen

# Beheer: Gegevensimport.
admin-data-import-title = Gegevensimport
admin-data-import-description = Importeer en synchroniseer gegevens uit externe bronnen
admin-data-import-notice = Imports kunnen meldingen naar betrokken gebruikers triggeren. Bestaande records worden bijgewerkt op basis van overeenkomende ID's.
admin-data-import-status-available = Beschikbaar
admin-data-import-status-coming-soon = Binnenkort beschikbaar
admin-data-import-status-beta = Bèta
admin-data-import-microsoft-title = Microsoft Graph
admin-data-import-microsoft-description = Importeer gegevens uit Microsoft 365, waaronder Azure AD, Intune en andere Microsoft-services
admin-data-import-csv-title = CSV-import
admin-data-import-csv-description = Importeer gegevens uit CSV-bestanden (apparaten, gebruikers en andere bronnen)
admin-data-import-api-title = API-integraties
admin-data-import-api-description = Maak verbinding met API's van derden om gegevens te importeren en synchroniseren
admin-data-import-ad-title = Active Directory
admin-data-import-ad-description = Importeer gegevens uit lokale Active Directory-servers

# Beheer: Authenticatieproviders.
admin-auth-providers-title = Authenticatieproviders
admin-auth-providers-env-notice-prefix = Authenticatieproviders worden geconfigureerd via omgevingsvariabelen in uw
admin-auth-providers-env-notice-suffix = bestand. Gebruik de knop "Configuratie valideren" om te controleren of elke provider correct is geconfigureerd.
admin-auth-providers-loading = Providers laden...
admin-auth-providers-default-badge = Standaard
admin-auth-providers-configured = Geconfigureerd
admin-auth-providers-not-configured = Niet geconfigureerd
admin-auth-providers-enabled = Ingeschakeld
admin-auth-providers-client-id = Client-ID
admin-auth-providers-tenant-id = Tenant-ID
admin-auth-providers-redirect-uri = Redirect-URI
admin-auth-providers-secret = Secret
admin-auth-providers-secret-not-set = Niet ingesteld
admin-auth-providers-env-label = Env:
admin-auth-providers-empty-title = Geen authenticatieproviders gevonden
admin-auth-providers-empty-description = Configureer authenticatieproviders in uw omgevingsvariabelen
admin-auth-providers-error-load = Kon authenticatieproviders niet laden
admin-auth-providers-error-validate = Configuratievalidatie mislukt

# Beheer: API-tokens.
admin-api-tokens-title = API-tokens
admin-api-tokens-description = Beheer API-tokens voor programmatische toegang
admin-api-tokens-create = Token aanmaken
admin-api-tokens-create-short = Aanmaken
admin-api-tokens-loading = Tokens laden...
admin-api-tokens-active-heading = Actieve tokens
admin-api-tokens-revoked-heading = Ingetrokken tokens
admin-api-tokens-user-prefix = Gebruiker:
admin-api-tokens-created-prefix = Aangemaakt { $when }
admin-api-tokens-expires-prefix = Verloopt { $when }
admin-api-tokens-no-expiration = Geen vervaldatum
admin-api-tokens-last-used-label = Laatst gebruikt:
admin-api-tokens-last-used-never = Nooit
admin-api-tokens-revoked-prefix = Ingetrokken { $when }
admin-api-tokens-revoke-title = Token intrekken
admin-api-tokens-error-load = Kon API-tokens niet laden
admin-api-tokens-error-create = Kon token niet aanmaken
admin-api-tokens-error-revoke = Kon token niet intrekken
admin-api-tokens-error-name-required = Tokennaam is vereist
admin-api-tokens-error-user-required = Selecteer een gebruiker
admin-api-tokens-revoke-success = Token ingetrokken
admin-api-tokens-modal-create-title = API-token aanmaken
admin-api-tokens-modal-name-label = Tokennaam
admin-api-tokens-modal-name-placeholder = bv. CI/CD-pipeline
admin-api-tokens-modal-name-hint = Een beschrijvende naam om dit token te identificeren
admin-api-tokens-modal-user-label = Gebruiker (handelt als)
admin-api-tokens-modal-user-placeholder = Selecteer een gebruiker...
admin-api-tokens-modal-user-hint = Het token krijgt dezelfde rechten als deze gebruiker
admin-api-tokens-modal-expiration-label = Vervaldatum
admin-api-tokens-modal-no-expiration-label = Geen vervaldatum
admin-api-tokens-modal-expires-days-suffix = dagen
admin-api-tokens-modal-expires-hint = Token vervalt na { $days } dagen
admin-api-tokens-modal-no-expiration-warning = Tokens zonder vervaldatum zijn minder veilig
admin-api-tokens-modal-cancel = Annuleren
admin-api-tokens-modal-creating = Aanmaken...
admin-api-tokens-created-title = Token aangemaakt
admin-api-tokens-created-warning = Kopieer dit token nu, het wordt niet opnieuw getoond!
admin-api-tokens-copied = Gekopieerd!
admin-api-tokens-copy-title = Naar klembord kopiëren
admin-api-tokens-bearer-hint-prefix = Gebruik dit token met de
admin-api-tokens-bearer-hint-suffix = header
admin-api-tokens-done = Klaar
admin-api-tokens-revoke-modal-title = Token intrekken
admin-api-tokens-revoke-confirm-prefix = Weet u zeker dat u het token wilt intrekken
admin-api-tokens-revoke-confirm-suffix = ?
admin-api-tokens-revoke-warning = Deze actie kan niet ongedaan worden gemaakt. Systemen die dit token gebruiken verliezen toegang.
admin-api-tokens-revoking = Intrekken...

# Beheer: SLA.
admin-sla-title = SLA
admin-sla-description = Werkkalenders en SLA-beleid voeden de SLA-indicator per ticket.
admin-sla-loading = Laden…
admin-sla-error-load = Kon SLA-configuratie niet laden
admin-sla-error-create = Aanmaken mislukt
admin-sla-error-delete = Verwijderen mislukt
admin-sla-error-update = Bijwerken mislukt
admin-sla-calendars-heading = Werkkalenders
admin-sla-policies-heading = SLA-beleid
admin-sla-col-name = Naam
admin-sla-col-tz = TZ
admin-sla-col-default = Standaard
admin-sla-col-response = Reactie
admin-sla-col-resolution = Oplossing
admin-sla-col-calendar = Kalender
admin-sla-default-badge = Standaard
admin-sla-set-default = Standaard instellen
admin-sla-delete = Verwijderen
admin-sla-calendar-delete-confirm = Deze kalender verwijderen? Beleid dat ernaar verwijst heeft een nieuwe kalender nodig.
admin-sla-policy-delete-confirm = Dit beleid verwijderen? Tickets die er momenteel onder vallen verliezen hun SLA-indicator totdat een ander beleid past. Dit kan niet ongedaan worden gemaakt.
admin-sla-new-calendar-heading = Nieuwe kalender
admin-sla-new-policy-heading = Nieuw beleid
admin-sla-field-name = Naam
admin-sla-field-tz = Tijdzone
admin-sla-field-calendar = Kalender
admin-sla-field-response = Reactie (minuten)
admin-sla-field-resolution = Oplossing (minuten)
admin-sla-field-priority = Prioriteitsfilter
admin-sla-placeholder-name = EU-support-uren
admin-sla-placeholder-tz = Europe/Amsterdam
admin-sla-policy-name-placeholder = Kritieke incidenten
admin-sla-schedule-hint = Standaardschema is ma-vr 9-17. Pas handmatig aan of breid hier later uit.
admin-sla-priority-any = Alle
admin-sla-priority-low = laag
admin-sla-priority-medium = gemiddeld
admin-sla-priority-high = hoog
admin-sla-workspace-default = Werkruimtestandaard
admin-sla-create = Aanmaken

# Beheer: Branding.
admin-branding-title = Branding
admin-branding-description = Pas het uiterlijk en de branding van de applicatie aan.
admin-branding-loading = Brandingconfiguratie laden...
admin-branding-general-heading = Algemene instellingen
admin-branding-app-name-label = Applicatienaam
admin-branding-app-name-placeholder = Nosdesk
admin-branding-app-name-hint = Deze naam verschijnt in de header en het browsertabblad
admin-branding-primary-color-label = Hoofdkleur
admin-branding-primary-color-hint = Hex-kleurcode voor accentelementen (bv. #2C80FF)
admin-branding-save = Instellingen opslaan
admin-branding-saving = Opslaan...
admin-branding-logo-heading = Logo
admin-branding-logo-dark-label = Logo donker thema
admin-branding-logo-light-label = Logo licht thema (optioneel)
admin-branding-logo-upload = Logo uploaden
admin-branding-logo-uploading = Uploaden...
admin-branding-logo-remove = Verwijderen
admin-branding-logo-formats = PNG, SVG, JPEG of WebP. Max 2MB.
admin-branding-logo-light-hint = Wordt gebruikt wanneer het lichte thema actief is. Valt terug op het hoofdlogo.
admin-branding-favicon-heading = Favicon
admin-branding-favicon-upload = Favicon uploaden
admin-branding-favicon-uploading = Uploaden...
admin-branding-favicon-formats = ICO, PNG of SVG. Aanbevolen formaat: 32x32 of 64x64 pixels.
admin-branding-preview-heading = Voorbeeld
admin-branding-primary-color-preview = Hoofdkleur
admin-branding-configured = Aangepaste branding geconfigureerd
admin-branding-success-saved = Brandinginstellingen opgeslagen
admin-branding-success-logo = Logo geüpload
admin-branding-success-logo-light = Logo licht thema geüpload
admin-branding-success-favicon = Favicon geüpload
admin-branding-success-removed = { $asset } verwijderd
admin-branding-error-load = Kon brandingconfiguratie niet laden
admin-branding-error-save = Kon brandinginstellingen niet opslaan
admin-branding-error-invalid-file = Ongeldig bestand
admin-branding-error-upload-logo = Kon logo niet uploaden
admin-branding-error-upload-logo-light = Kon logo voor licht thema niet uploaden
admin-branding-error-upload-favicon = Kon favicon niet uploaden
admin-branding-error-delete = Kon { $asset } niet verwijderen
admin-branding-asset-logo = Logo
admin-branding-asset-logo-light = Logo licht thema
admin-branding-asset-favicon = Favicon
admin-branding-confirm-title = { $asset } verwijderen?
admin-branding-confirm-message = Hiermee wordt de geüploade afbeelding verwijderd. U kunt opnieuw uploaden, maar het vorige bestand is niet herstelbaar.
admin-branding-confirm-remove = Verwijderen

# Beheer: Back-up en herstel.
admin-backup-title = Back-up en herstel
admin-backup-description = Exporteer en herstel systeemgegevens en bijlagen
admin-backup-create-heading = Back-up maken
admin-backup-create-description = Exporteer alle systeemgegevens en bijlagen naar een ZIP-archief
admin-backup-include-sensitive-label = Gevoelige gegevens opnemen
admin-backup-include-sensitive-description = Bevat wachtwoorden, MFA-secrets en authenticatie-tokens (versleuteld met wachtwoord)
admin-backup-encryption-warning = Gevoelige gegevens worden versleuteld. Als u het wachtwoord verliest, zijn de gegevens niet herstelbaar.
admin-backup-encryption-password-label = Versleutelwachtwoord
admin-backup-encryption-password-placeholder = Voer versleutelwachtwoord in
admin-backup-confirm-password-label = Wachtwoord bevestigen
admin-backup-confirm-password-placeholder = Bevestig versleutelwachtwoord
admin-backup-passwords-no-match = Wachtwoorden komen niet overeen
admin-backup-create-button = Back-up maken
admin-backup-creating = Back-up maken...
admin-backup-recent-heading = Recente back-ups
admin-backup-refresh = Vernieuwen
admin-backup-empty = Nog geen back-ups. Maak hierboven uw eerste back-up.
admin-backup-encrypted-badge = Versleuteld
admin-backup-creating-status = Maken...
admin-backup-download-title = Downloaden
admin-backup-delete-title = Verwijderen
admin-backup-docs-heading = Documentatie exporteren naar Markdown
admin-backup-docs-description = Exporteer alle documentatiepagina's als markdown-bestanden in een ZIP-archief
admin-backup-docs-export = Exporteren als Markdown
admin-backup-docs-exporting = Exporteren { $current }/{ $total }...
admin-backup-docs-preparing = Voorbereiden...
admin-backup-docs-error = Kon documentatie niet exporteren. Bekijk de console voor details.
admin-backup-restore-heading = Herstellen vanaf back-up
admin-backup-restore-description = Upload een back-upbestand om systeemgegevens en bijlagen te herstellen
admin-backup-restore-dnd = Sleep een back-upbestand hierheen, of
admin-backup-restore-browse = blader om een bestand te selecteren
admin-backup-details-heading = Back-upgegevens
admin-backup-detail-created = Aangemaakt:
admin-backup-detail-version = Versie:
admin-backup-detail-files = Bestanden:
admin-backup-detail-size = Grootte:
admin-backup-detail-tables = Tabellen:
admin-backup-warnings-heading = Waarschuwingen
admin-backup-decryption-password-label = Ontsleutelwachtwoord
admin-backup-decryption-password-placeholder = Voer het versleutelwachtwoord van de back-up in
admin-backup-restore-warning = Herstellen vervangt bestaande bestanden. Deze actie kan niet ongedaan worden gemaakt.
admin-backup-restore-button = Bestanden herstellen
admin-backup-restoring = Herstellen...
admin-backup-cancel = Annuleren
admin-backup-restore-not-zip = Selecteer een .zip-back-upbestand
admin-backup-upload-error = Kon back-upbestand niet uploaden
admin-backup-restore-success = Herstel voltooid: { $files } bestanden hersteld. { $message }
admin-backup-restore-error = Herstel mislukt. Bekijk de console voor details.
admin-backup-delete-confirm-title = Deze back-up verwijderen?
admin-backup-delete-confirm-message = Het back-upbestand wordt permanent verwijderd.
admin-backup-delete-confirm-label = Verwijderen

# Beheer: Toewijzingsregels.
admin-assignment-rules-title = Toewijzingsregels
admin-assignment-rules-description = Configureer automatische tickettoewijzing op basis van regels
admin-assignment-rules-new = Nieuwe regel
admin-assignment-rules-info = Regels worden in prioriteitsvolgorde geëvalueerd (boven naar onder). De eerste passende regel wint. Tickets met een bestaande toegewezene worden niet automatisch toegewezen.
admin-assignment-rules-loading = Regels laden...
admin-assignment-rules-active = Actief
admin-assignment-rules-inactive = Inactief
admin-assignment-rules-target-none = Niet geconfigureerd
admin-assignment-rules-trigger-both = Beide triggers
admin-assignment-rules-trigger-create = Bij aanmaken
admin-assignment-rules-trigger-category = Bij categoriewijziging
admin-assignment-rules-trigger-none = Geen triggers
admin-assignment-rules-assigned-count = { $count } toegewezen
admin-assignment-rules-move-up = Omhoog (hogere prioriteit)
admin-assignment-rules-move-down = Omlaag (lagere prioriteit)
admin-assignment-rules-toggle-deactivate = Regel deactiveren
admin-assignment-rules-toggle-activate = Regel activeren
admin-assignment-rules-edit = Regel bewerken
admin-assignment-rules-delete = Regel verwijderen
admin-assignment-rules-create-action = Regel aanmaken
admin-assignment-rules-error-load = Kon toewijzingsregels niet laden
admin-assignment-rules-error-name = Regelnaam is vereist
admin-assignment-rules-error-user = Selecteer een doelgebruiker
admin-assignment-rules-error-group = Selecteer een doelgroep
admin-assignment-rules-error-save = Kon regel niet opslaan
admin-assignment-rules-error-update = Kon regel niet bijwerken
admin-assignment-rules-error-delete = Kon regel niet verwijderen
admin-assignment-rules-error-reorder = Kon regels niet opnieuw ordenen
admin-assignment-rules-success-create = Regel aangemaakt
admin-assignment-rules-success-update = Regel bijgewerkt
admin-assignment-rules-success-delete = Regel verwijderd
admin-assignment-rules-method-direct-label = Directe gebruiker
admin-assignment-rules-method-direct-description = Direct toewijzen aan een specifieke gebruiker
admin-assignment-rules-method-round-robin-label = Round-Robin (groep)
admin-assignment-rules-method-round-robin-description = Toewijzing gelijkmatig rouleren tussen groepsleden
admin-assignment-rules-method-random-label = Willekeurig (groep)
admin-assignment-rules-method-random-description = Selecteer willekeurig een groepslid voor elk ticket
admin-assignment-rules-method-queue-label = Groepswachtrij
admin-assignment-rules-method-queue-description = Toewijzen aan de groepswachtrij (gebruikers claimen tickets)
admin-assignment-rules-modal-create-title = Toewijzingsregel aanmaken
admin-assignment-rules-modal-edit-title = Toewijzingsregel bewerken
admin-assignment-rules-modal-name-label = Regelnaam
admin-assignment-rules-modal-name-placeholder = bv. IT-support round-robin
admin-assignment-rules-modal-description-label = Beschrijving (optioneel)
admin-assignment-rules-modal-description-placeholder = Beschrijf wat deze regel doet...
admin-assignment-rules-modal-method-label = Toewijzingsmethode
admin-assignment-rules-modal-user-label = Doelgebruiker
admin-assignment-rules-modal-user-placeholder = Selecteer een gebruiker...
admin-assignment-rules-modal-group-label = Doelgroep
admin-assignment-rules-modal-group-placeholder = Selecteer een groep...
admin-assignment-rules-modal-group-members = { $count } leden
admin-assignment-rules-modal-category-label = Categoriefilter (optioneel)
admin-assignment-rules-modal-category-all = Alle categorieën
admin-assignment-rules-modal-category-hint = Wijs alleen tickets met deze categorie toe (leeg = alle)
admin-assignment-rules-modal-triggers-label = Triggers
admin-assignment-rules-modal-trigger-create-label = Wanneer een ticket wordt aangemaakt
admin-assignment-rules-modal-trigger-category-label = Wanneer de categorie van een ticket verandert
admin-assignment-rules-modal-active-label = Regel is actief
admin-assignment-rules-modal-cancel = Annuleren
admin-assignment-rules-modal-saving = Opslaan...
admin-assignment-rules-modal-update = Regel bijwerken
admin-assignment-rules-modal-create = Regel aanmaken
admin-assignment-rules-delete-title = Toewijzingsregel verwijderen
admin-assignment-rules-delete-message = Weet u zeker dat u de regel "{ $name }" wilt verwijderen? Deze actie kan niet ongedaan worden gemaakt.
admin-assignment-rules-delete-cancel = Annuleren
admin-assignment-rules-delete-confirm = Verwijderen
admin-assignment-rules-deleting = Verwijderen...

# Admin: Categories (CategoriesManagementView).
admin-categories-title = Categorieën
admin-categories-description = Beheer ticketcategorieën en groepszichtbaarheid
admin-categories-new = Nieuwe categorie
admin-categories-info = Categorieën zonder groepsbeperking zijn zichtbaar voor alle gebruikers. Wijs groepen toe om de zichtbaarheid te beperken.
admin-categories-loading = Categorieën laden...
admin-categories-search-placeholder = Categorieën zoeken...
admin-categories-filter-all = Alle categorieën
admin-categories-filter-active = Alleen actief
admin-categories-filter-inactive = Alleen inactief
admin-categories-filter-public = Alleen openbaar
admin-categories-filter-restricted = Alleen beperkt
admin-categories-sort-custom = Aangepaste volgorde
admin-categories-sort-name = Naam
admin-categories-sort-ascending = Oplopend
admin-categories-sort-descending = Aflopend
admin-categories-drag-handle = Sleep om te herordenen
admin-categories-badge-public = Openbaar
admin-categories-badge-groups = { $count ->
    [one] { $count } groep
   *[other] { $count } groepen
    }
admin-categories-badge-inactive = Inactief
admin-categories-groups-more = +{ $count } meer
admin-categories-action-deactivate = Deactiveren
admin-categories-action-activate = Activeren
admin-categories-action-edit = Categorie bewerken
admin-categories-action-delete = Categorie verwijderen
admin-categories-no-search-results = Geen categorieën gevonden voor "{ $query }"
admin-categories-no-filter-results = Geen categorieën komen overeen met het huidige filter
admin-categories-empty-action = Categorie aanmaken
admin-categories-modal-create-title = Categorie aanmaken
admin-categories-modal-edit-title = Categorie bewerken
admin-categories-modal-name-label = Naam
admin-categories-modal-name-placeholder = Voer categorienaam in
admin-categories-modal-description-label = Beschrijving
admin-categories-modal-description-placeholder = Optionele beschrijving
admin-categories-modal-icon-label = Pictogram
admin-categories-modal-color-label = Kleur
admin-categories-modal-active-label = Actief
admin-categories-modal-visibility-label = Zichtbaar voor groepen
admin-categories-modal-visibility-hint = (laat leeg voor openbaar)
admin-categories-modal-visibility-toggle-aria = Zichtbaarheid wisselen voor { $name }
admin-categories-modal-group-members = { $count } leden
admin-categories-modal-no-groups = Geen groepen beschikbaar.
admin-categories-modal-create-groups-link = Groepen aanmaken
admin-categories-modal-create-groups-suffix = eerst.
admin-categories-modal-cancel = Annuleren
admin-categories-modal-save = Wijzigingen opslaan
admin-categories-modal-create = Categorie aanmaken
admin-categories-delete-title = Categorie verwijderen
admin-categories-delete-message = Weet u zeker dat u de categorie "{ $name }" wilt verwijderen? Tickets die deze categorie gebruiken, krijgen hun categorie gewist.
admin-categories-delete-cancel = Annuleren
admin-categories-delete-confirm = Categorie verwijderen
admin-categories-error-name-required = Categorienaam is verplicht
admin-categories-error-load = Kon categorieën niet laden
admin-categories-error-reorder = Kon categorieën niet herordenen
admin-categories-error-save = Kon categorie niet opslaan
admin-categories-error-update = Kon categorie niet bijwerken
admin-categories-error-delete = Kon categorie niet verwijderen
admin-categories-success-create = Categorie succesvol aangemaakt
admin-categories-success-update = Categorie succesvol bijgewerkt
admin-categories-success-delete = Categorie succesvol verwijderd

# Admin: Email channels (ChannelsEmailSettingsView).
admin-channels-email-title = E-mailinname
admin-channels-email-description = Vraag een supportmailbox op via IMAP en zet inkomende berichten om in tickets. Reacties van technici worden via dezelfde thread teruggestuurd.
admin-channels-email-loading = Kanaal laden...
admin-channels-email-status-heading = Status
admin-channels-email-status-subtitle = Live overzicht van wat de innameworker als laatste deed.
admin-channels-email-status-enabled = Ingeschakeld
admin-channels-email-status-disabled = Uitgeschakeld
admin-channels-email-status-last-polled = Laatst opgevraagd
admin-channels-email-status-never = nooit
admin-channels-email-status-last-uid = Laatst geziene UID
admin-channels-email-status-uid-validity = UIDVALIDITY
admin-channels-email-status-last-error = Laatste fout
admin-channels-email-status-last-error-hint = De worker blijft het opnieuw proberen met exponentiële vertraging. Los het onderliggende probleem op en het wordt bij de volgende geslaagde poll gewist.
admin-channels-email-form-heading-edit = Configuratie
admin-channels-email-form-heading-create = Mailbox koppelen
admin-channels-email-form-subtitle = Alleen IMAP over TLS. Zie de geavanceerde optie hieronder voor zelf-gehoste testservers met een zelfondertekend certificaat.
admin-channels-email-toggle-enabled-label = Ingeschakeld
admin-channels-email-toggle-enabled-description = Uitgeschakeld stopt de worker met pollen, maar de opgeslagen configuratie en inloggegevens blijven bewaard.
admin-channels-email-field-name-label = Weergavenaam
admin-channels-email-field-name-placeholder = bijv. Supportinbox
admin-channels-email-field-name-hint = Alleen zichtbaar in de adminomgeving. Klanten zien dit nooit.
admin-channels-email-field-host-label = IMAP-host
admin-channels-email-field-host-placeholder = imap.example.com
admin-channels-email-field-port-label = Poort
admin-channels-email-field-port-hint = 993 voor IMAPS. 143 vereist STARTTLS (nog niet ondersteund).
admin-channels-email-field-username-label = Gebruikersnaam
admin-channels-email-field-username-placeholder = support@example.com
admin-channels-email-field-mailbox-label = Mailbox
admin-channels-email-field-mailbox-placeholder = INBOX
admin-channels-email-field-mailbox-hint = Gmail-gebruikers willen mogelijk "[Gmail]/All Mail".
admin-channels-email-field-reply-domain-label = Antwoorddomein
admin-channels-email-field-reply-domain-placeholder = example.com
admin-channels-email-field-reply-domain-hint = Gebruikt om Message-ID's op uitgaande antwoorden te stempelen zodat het antwoord van de klant terug threadt in hetzelfde ticket. Meestal hetzelfde domein als de gebruikersnaam.
admin-channels-email-field-password-label = Wachtwoord
admin-channels-email-field-password-keep-existing = (leeg laten om bestaand te behouden)
admin-channels-email-field-password-placeholder-stored = •••••••••• (opgeslagen)
admin-channels-email-field-password-placeholder-new = App-wachtwoord of accountwachtwoord
admin-channels-email-remove-password = Opgeslagen wachtwoord verwijderen
admin-channels-email-removing-password = Verwijderen...
admin-channels-email-advanced = Geavanceerd
admin-channels-email-toggle-insecure-label = TLS-certificaatverificatie overslaan
admin-channels-email-toggle-insecure-description = ALLEEN voor Greenmail of zelf-gehoste testservers met een zelfondertekend certificaat. Laat dit uit staan in productie.
admin-channels-email-test = Verbinding testen
admin-channels-email-testing = Testen...
admin-channels-email-test-connected = Verbonden
admin-channels-email-test-failed = Mislukt
admin-channels-email-test-unknown-error = Onbekende fout
admin-channels-email-delete = Verwijderen
admin-channels-email-deleting = Verwijderen...
admin-channels-email-save = Wijzigingen opslaan
admin-channels-email-saving = Opslaan...
admin-channels-email-create = Kanaal aanmaken
admin-channels-email-creating = Aanmaken...
admin-channels-email-clear-credential-title = Opgeslagen wachtwoord verwijderen?
admin-channels-email-clear-credential-message = De worker stopt met authenticeren totdat een nieuw wachtwoord is opgeslagen.
admin-channels-email-clear-credential-confirm = Verwijderen
admin-channels-email-delete-title = Dit e-mailkanaal verwijderen?
admin-channels-email-delete-message = Tickets die er al uit zijn voortgekomen blijven intact, maar er worden geen nieuwe berichten meer ingenomen. Dit kan niet ongedaan worden gemaakt.
admin-channels-email-delete-confirm = Kanaal verwijderen
admin-channels-email-relative-seconds = { $count } s geleden
admin-channels-email-relative-minutes = { $count } min geleden
admin-channels-email-relative-hours = { $count } u geleden
admin-channels-email-relative-days = { $count } d geleden
admin-channels-email-error-load = Kon e-mailkanaal niet laden
admin-channels-email-success-update = Kanaal bijgewerkt
admin-channels-email-success-create = Kanaal aangemaakt
admin-channels-email-success-password-removed = Wachtwoord verwijderd
admin-channels-email-success-delete = Kanaal verwijderd

# Admin: Microsoft Graph (gegevensimport)
admin-msgraph-back = Terug naar gegevensimport
admin-msgraph-title = Microsoft Graph
admin-msgraph-subtitle = Beheer gegevenssynchronisatie vanuit Microsoft 365-services
admin-msgraph-sync-action = Gegevens synchroniseren
admin-msgraph-syncing = Synchroniseren...
admin-msgraph-api-name = Microsoft Graph API
admin-msgraph-status-connected = Verbonden
admin-msgraph-status-disconnected = Niet verbonden
admin-msgraph-status-connecting = Verbinden...
admin-msgraph-status-error = Fout
admin-msgraph-config-valid = Geconfigureerd
admin-msgraph-config-invalid = Niet geconfigureerd
admin-msgraph-field-client-id = Client ID
admin-msgraph-field-tenant-id = Tenant ID
admin-msgraph-field-secret = Secret
admin-msgraph-field-not-set = Niet ingesteld
admin-msgraph-secret-configured = Geconfigureerd
admin-msgraph-secret-not-set = Niet ingesteld
admin-msgraph-last-synced = Laatst gesynchroniseerd:
admin-msgraph-missing-config = Vereiste configuratie ontbreekt:
admin-msgraph-env-label = Env:
admin-msgraph-progress-title = Synchroniseren
admin-msgraph-progress-step = Stap { $current } van { $total }
admin-msgraph-progress-status-running = bezig
admin-msgraph-progress-status-starting = starten
admin-msgraph-progress-status-completed = voltooid
admin-msgraph-progress-status-completed-with-errors = Voltooid met fouten
admin-msgraph-progress-status-cancelling = annuleren
admin-msgraph-progress-status-cancelled = geannuleerd
admin-msgraph-progress-status-error = fout
admin-msgraph-cancel = Annuleren
admin-msgraph-monitor = Volgen
admin-msgraph-delta-badge = Delta
admin-msgraph-last-sync-title = Laatste synchronisatie
admin-msgraph-last-sync-status-completed = Voltooid
admin-msgraph-last-sync-status-completed-with-errors = Voltooid met fouten
admin-msgraph-last-sync-status-error = Fout
admin-msgraph-last-sync-status-cancelled = Geannuleerd
admin-msgraph-last-sync-type = Type
admin-msgraph-last-sync-type-delta = Delta
admin-msgraph-last-sync-type-full = Volledig
admin-msgraph-last-sync-started = Gestart
admin-msgraph-last-sync-duration = Duur
admin-msgraph-last-sync-items-processed = Items verwerkt
admin-msgraph-last-sync-cancelled-value = Geannuleerd
admin-msgraph-last-sync-failed-value = Mislukt
admin-msgraph-modal-title = Gegevens synchroniseren vanuit Microsoft Graph
admin-msgraph-modal-description = Selecteer de gegevens die u uit Microsoft Graph wilt importeren:
admin-msgraph-entity-users-name = Gebruikers
admin-msgraph-entity-users-description = Importeer gebruikersaccounts en profielen uit Microsoft Entra ID
admin-msgraph-entity-devices-name = Apparaten
admin-msgraph-entity-devices-description = Importeer beheerde apparaten uit Microsoft Intune met gebruikerstoewijzingen
admin-msgraph-entity-groups-name = Groepen
admin-msgraph-entity-groups-description = Importeer beveiligings- en distributiegroepen uit Microsoft Entra ID
admin-msgraph-modal-info = Synchronisatie haalt de nieuwste gegevens op uit Microsoft-services. Dit kan enkele minuten duren afhankelijk van het volume.
admin-msgraph-results-title = Synchronisatieresultaten
admin-msgraph-results-items = { $processed } / { $total } items
admin-msgraph-results-percent = ({ $percent }%)
admin-msgraph-results-more-errors = ... en nog { $count } fouten
admin-msgraph-results-total-processed = Totaal verwerkt:
admin-msgraph-results-total-processed-value = { $count } items
admin-msgraph-results-total-errors = Totaal aantal fouten:
admin-msgraph-full-sync = Volledige synchronisatie
admin-msgraph-start-sync = Synchronisatie starten
admin-msgraph-starting = Starten...
admin-msgraph-sync-type-users = Gebruikersaccounts
admin-msgraph-sync-type-profile-photos = Profielfoto's
admin-msgraph-sync-type-devices = Beheerde apparaten
admin-msgraph-sync-type-groups = Beveiligingsgroepen
admin-msgraph-time-just-now = Zojuist
admin-msgraph-time-minutes = { $count } min geleden
admin-msgraph-time-hours = { $count } u geleden
admin-msgraph-time-days = { $count } d geleden
admin-msgraph-duration-seconds = { $seconds }s
admin-msgraph-duration-minutes = { $minutes }m { $seconds }s
admin-msgraph-duration-hours = { $hours }u { $minutes }m
admin-msgraph-error-validate-config = Configuratie kon niet worden gevalideerd
admin-msgraph-error-fetch-status = Verbindingsstatus kon niet worden opgehaald
admin-msgraph-error-start-sync = Synchronisatie kon niet worden gestart
admin-msgraph-error-cancel-sync = Synchronisatie kon niet worden geannuleerd
admin-msgraph-success-sync-started = Synchronisatie gestart
admin-msgraph-success-cancel-requested = Annulering van synchronisatie aangevraagd

# Admin: Plugin-register (bladeren en installeren)
admin-plugins-registry-back = Geïnstalleerde plugins
admin-plugins-registry-title = Plugin-register
admin-plugins-registry-subtitle-before = Blader en installeer plugins gepubliceerd op
admin-plugins-registry-subtitle-after = . Handtekeningen worden geverifieerd tegen de Nosdesk-hoofdsleutel voordat een bundel wordt uitgevoerd.
admin-plugins-registry-refresh = Vernieuwen
admin-plugins-registry-refreshing = Vernieuwen
admin-plugins-registry-loading = Register laden...
admin-plugins-registry-disabled-title = Register-synchronisatie is uitgeschakeld
admin-plugins-registry-disabled-description-sideload = Deze instantie heeft NOSDESK_REGISTRY_URL leeg ingesteld, dus de gepubliceerde plugincatalogus wordt niet opgehaald. Je kunt nog steeds een ondertekende zip handmatig installeren.
admin-plugins-registry-disabled-description-cli = Deze instantie heeft NOSDESK_REGISTRY_URL leeg ingesteld, dus de gepubliceerde plugincatalogus wordt niet opgehaald. Gebruik de CLI om lokaal-ondertekende plugins te installeren.
admin-plugins-registry-disabled-action = Ondertekende zip handmatig installeren
admin-plugins-registry-pending-title = Register wordt gesynchroniseerd
admin-plugins-registry-pending-description = De instantie haalt de gepubliceerde plugincatalogus op. Dit is meestal binnen enkele seconden na opstarten klaar.
admin-plugins-registry-failed-title = Register-synchronisatie mislukt
admin-plugins-registry-failed-description = { $reason }. Probeer nu opnieuw om opnieuw op te halen, of wacht op de volgende geplande poging.
admin-plugins-registry-retry-now = Nu opnieuw proberen
admin-plugins-registry-search-label = Plugins zoeken
admin-plugins-registry-search-placeholder = Plugins zoeken
admin-plugins-registry-filter-aria = Register filteren
admin-plugins-registry-trust-tier = Vertrouwensniveau
admin-plugins-registry-tier-official = Officieel
admin-plugins-registry-tier-verified = Geverifieerd
admin-plugins-registry-tier-community = Community
admin-plugins-registry-tier-local = Lokaal
admin-plugins-registry-reset-filters = Filters resetten
admin-plugins-registry-snapshot-fetched = Momentopname opgehaald { $relative }
admin-plugins-registry-result-count = { $filtered } van { $total } { $total ->
    [one] plugin
   *[other] plugins
   }
admin-plugins-registry-no-matches = Geen plugins komen overeen met deze filters.
admin-plugins-registry-installed-badge = Geïnstalleerd
admin-plugins-registry-manage = Beheren
admin-plugins-registry-install = Installeren
admin-plugins-registry-installing = Installeren...
admin-plugins-registry-sr-plugin-name = Pluginnaam
admin-plugins-registry-sr-publisher = Uitgever
admin-plugins-registry-sr-homepage = Website
admin-plugins-registry-by-publisher = door { $publisher }
admin-plugins-registry-homepage-link = Website
admin-plugins-registry-publisher-nosdesk = Nosdesk
admin-plugins-registry-publisher-unknown = Onbekende uitgever
admin-plugins-registry-modal-title = { $name } installeren?
admin-plugins-registry-community-warning-strong = Community-plugin.
admin-plugins-registry-community-warning-body = Nosdesk staat niet in voor de veiligheid van community-plugins buiten de verificatie van de handtekening van de uitgever. Bekijk de broncode voordat je je gegevens eraan toevertrouwt.
admin-plugins-registry-field-publisher = Uitgever
admin-plugins-registry-field-fingerprint = Vingerafdruk
admin-plugins-registry-field-version = Versie
admin-plugins-registry-type-to-confirm-before = Typ
admin-plugins-registry-type-to-confirm-after = om te bevestigen
admin-plugins-registry-cancel = Annuleren
admin-plugins-registry-error-load = Register laden mislukt.
admin-plugins-registry-error-refresh = Opnieuw proberen van register-synchronisatie mislukt.
admin-plugins-registry-error-confirm-name = Typ de pluginnaam exact om de installatie te bevestigen.
admin-plugins-registry-error-install = Installatie mislukt.
admin-plugins-registry-success-installed = { $name } v{ $version } geïnstalleerd
admin-plugins-registry-relative-just-now = zojuist
admin-plugins-registry-relative-minutes = { $count } min geleden
admin-plugins-registry-relative-hours = { $count } u geleden
admin-plugins-registry-relative-days = { $count ->
    [one] { $count } dag geleden
   *[other] { $count } dagen geleden
   }

# Admin: Webhooks (beheer uitgaande gebeurteniaflevering)
admin-webhooks-title = Webhooks
admin-webhooks-subtitle = Beheer webhooks voor externe integraties
admin-webhooks-create = Webhook aanmaken
admin-webhooks-create-short = Aanmaken
admin-webhooks-loading = Webhooks laden...
admin-webhooks-section-active = Actieve webhooks
admin-webhooks-section-disabled = Uitgeschakelde webhooks
admin-webhooks-status-active = Actief
admin-webhooks-status-warning = Waarschuwing
admin-webhooks-status-failing = Mislukt
admin-webhooks-status-disabled = Uitgeschakeld
admin-webhooks-failure-count = { $count ->
    [one] { $count } fout
   *[other] { $count } fouten
   }
admin-webhooks-meta-secret = Geheim:
admin-webhooks-meta-events = { $count ->
    [one] { $count } gebeurtenis
   *[other] { $count } gebeurtenissen
   }
admin-webhooks-meta-last-triggered = Laatst geactiveerd: { $when }
admin-webhooks-meta-never = Nooit
admin-webhooks-action-send-test = Testgebeurtenis verzenden
admin-webhooks-action-view-deliveries = Afleveringen bekijken
admin-webhooks-action-edit = Webhook bewerken
admin-webhooks-action-delete = Webhook verwijderen
admin-webhooks-modal-create-title = Webhook aanmaken
admin-webhooks-modal-edit-title = Webhook bewerken
admin-webhooks-modal-secret-title = Webhook aangemaakt
admin-webhooks-modal-regenerate-title = Geheim opnieuw genereren
admin-webhooks-modal-delete-title = Webhook verwijderen
admin-webhooks-modal-deliveries-title = Afleveringsgeschiedenis - { $name }
admin-webhooks-form-name-label = Naam
admin-webhooks-form-name-placeholder = bijv. Slack-meldingen
admin-webhooks-form-url-label = Payload-URL
admin-webhooks-form-url-placeholder = https://example.com/webhook
admin-webhooks-form-url-hint = POST-verzoeken worden naar deze URL gestuurd
admin-webhooks-form-events-label = Gebeurtenissen
admin-webhooks-form-events-hint = Kies welke gebeurtenissen deze webhook activeren
admin-webhooks-form-events-count = { $selected }/{ $total }
admin-webhooks-form-headers-label = Aangepaste headers
admin-webhooks-form-headers-add = + Header toevoegen
admin-webhooks-form-headers-name-placeholder = Headernaam
admin-webhooks-form-headers-value-placeholder = Waarde
admin-webhooks-form-headers-empty = Geen aangepaste headers
admin-webhooks-form-enabled-label = Ingeschakeld
admin-webhooks-form-enabled-description = Webhook ontvangt gebeurtenissen wanneer ingeschakeld
admin-webhooks-form-secret-label = Geheim
admin-webhooks-form-secret-regenerate = Opnieuw genereren
admin-webhooks-form-cancel = Annuleren
admin-webhooks-form-create = Webhook aanmaken
admin-webhooks-form-creating = Aanmaken...
admin-webhooks-form-save = Wijzigingen opslaan
admin-webhooks-form-saving = Opslaan...
admin-webhooks-secret-warning = Kopieer dit geheim nu, het wordt niet opnieuw getoond!
admin-webhooks-secret-helper-before = Gebruik dit geheim om webhook-handtekeningen te verifiëren via de header
admin-webhooks-secret-helper-after = { "" }
admin-webhooks-secret-copy = Naar klembord kopiëren
admin-webhooks-secret-copied = Gekopieerd!
admin-webhooks-secret-done = Klaar
admin-webhooks-regenerate-question = Weet je zeker dat je het geheim voor { $name } opnieuw wilt genereren?
admin-webhooks-regenerate-warning = Het huidige geheim wordt ongeldig. Je moet je integratie bijwerken met het nieuwe geheim.
admin-webhooks-regenerate-confirm = Opnieuw genereren
admin-webhooks-regenerate-running = Opnieuw genereren...
admin-webhooks-delete-question = Weet je zeker dat je de webhook { $name } wilt verwijderen?
admin-webhooks-delete-warning = Deze actie kan niet ongedaan worden gemaakt. Alle afleveringsgeschiedenis gaat verloren.
admin-webhooks-delete-confirm = Webhook verwijderen
admin-webhooks-delete-running = Verwijderen...
admin-webhooks-deliveries-loading = Afleveringen laden...
admin-webhooks-deliveries-empty-title = Nog geen afleveringen
admin-webhooks-deliveries-empty-description = Afleveringen verschijnen hier zodra er gebeurtenissen worden geactiveerd
admin-webhooks-deliveries-status-error = Fout
admin-webhooks-deliveries-status-pending = In behandeling
admin-webhooks-deliveries-attempt = Poging { $number }
admin-webhooks-deliveries-duration = { $ms } ms
admin-webhooks-deliveries-close = Sluiten
admin-webhooks-error-name-required = Webhooknaam is verplicht
admin-webhooks-error-url-required = URL is verplicht
admin-webhooks-error-event-required = Selecteer minstens één gebeurtenis
admin-webhooks-error-load = Webhooks laden mislukt
admin-webhooks-error-create = Webhook aanmaken mislukt
admin-webhooks-error-update = Webhook bijwerken mislukt
admin-webhooks-error-delete = Webhook verwijderen mislukt
admin-webhooks-error-test = Testgebeurtenis verzenden mislukt
admin-webhooks-error-regenerate = Opnieuw genereren van geheim mislukt
admin-webhooks-success-update = Webhook bijgewerkt
admin-webhooks-success-delete = Webhook verwijderd
admin-webhooks-success-test = Testgebeurtenis verzonden naar webhook
admin-webhooks-success-regenerate = Geheim opnieuw gegenereerd, raadpleeg de webhookafleveringen voor de nieuwe handtekening
admin-webhooks-category-tickets = Tickets
admin-webhooks-category-comments = Reacties
admin-webhooks-category-attachments = Bijlagen
admin-webhooks-category-devices = Apparaten
admin-webhooks-category-projects = Projecten
admin-webhooks-category-documentation = Documentatie
admin-webhooks-category-users = Gebruikers
admin-webhooks-event-ticket-created = Ticket aangemaakt
admin-webhooks-event-ticket-updated = Ticket bijgewerkt
admin-webhooks-event-ticket-deleted = Ticket verwijderd
admin-webhooks-event-ticket-linked = Ticket gekoppeld
admin-webhooks-event-ticket-unlinked = Ticket ontkoppeld
admin-webhooks-event-comment-added = Reactie toegevoegd
admin-webhooks-event-comment-deleted = Reactie verwijderd
admin-webhooks-event-attachment-added = Bijlage toegevoegd
admin-webhooks-event-attachment-deleted = Bijlage verwijderd
admin-webhooks-event-device-linked = Apparaat gekoppeld
admin-webhooks-event-device-unlinked = Apparaat ontkoppeld
admin-webhooks-event-device-updated = Apparaat bijgewerkt
admin-webhooks-event-project-assigned = Project toegewezen
admin-webhooks-event-project-unassigned = Projecttoewijzing opgeheven
admin-webhooks-event-documentation-updated = Documentatie bijgewerkt
admin-webhooks-event-user-created = Gebruiker aangemaakt
admin-webhooks-event-user-updated = Gebruiker bijgewerkt
admin-webhooks-event-user-deleted = Gebruiker verwijderd

# Users list (UsersListView): people directory with role filter,
# bulk role change, and bulk delete.
user-mgmt-search-placeholder = Gebruikers zoeken...
user-mgmt-item-label = gebruiker
user-mgmt-filter-all-roles = Alle rollen
user-mgmt-role-admin = Beheerder
user-mgmt-role-technician = Technicus
user-mgmt-role-user = Gebruiker
user-mgmt-column-user = Gebruiker
user-mgmt-column-role = Rol
user-mgmt-column-tickets = Tickets
user-mgmt-column-devices = Apparaten
user-mgmt-column-joined = Lid sinds
user-mgmt-invite-action = Gebruiker uitnodigen
user-mgmt-mobile-tickets = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
user-mgmt-mobile-devices = { $count ->
    [one] { $count } apparaat
   *[other] { $count } apparaten
   }
user-mgmt-bulk-role = Rol
user-mgmt-bulk-delete = Verwijderen
user-mgmt-bulk-delete-count = { $count } verwijderen
user-mgmt-bulk-action-error = Bulkactie mislukt. Probeer het opnieuw.
user-mgmt-role-modal-title = Rol instellen
user-mgmt-role-modal-body = { $count ->
    [one] Rol bijwerken voor { $count } gebruiker
   *[other] Rol bijwerken voor { $count } gebruikers
   }

# Gebruikersprofiel (UserProfileView): gebruikersdetail,
# aanmaakformulier, apparaten- en groepenpanelen en tickets.
user-profile-document-title = Profiel van { $name } | Nosdesk
user-profile-back-to-users = Terug naar gebruikers
user-profile-action-profile-settings = Profielinstellingen
user-profile-action-user-settings = Gebruikersinstellingen
user-profile-create-title = Nieuwe gebruiker aanmaken
user-profile-create-subtitle = Voeg een nieuwe gebruiker toe aan je organisatie
user-profile-section-basic-info = Basisgegevens
user-profile-field-name = Volledige naam
user-profile-field-name-placeholder = Voer volledige naam in
user-profile-field-email = E-mailadres
user-profile-field-email-placeholder = gebruiker@voorbeeld.nl
user-profile-field-role = Rol
user-profile-field-role-placeholder = Selecteer een rol
user-profile-role-user = Gebruiker
user-profile-role-technician = Technicus
user-profile-role-admin = Beheerder
user-profile-field-pronouns = Voornaamwoorden
user-profile-field-pronouns-placeholder = bv. hij/hem, zij/haar, die/diens
user-profile-section-account-setup = Accountinstelling
user-profile-smtp-warning-title = E-mail niet geconfigureerd
user-profile-smtp-warning-body = Je moet handmatig een wachtwoord instellen, want e-mailuitnodigingen zijn niet beschikbaar.
user-profile-setup-method = Instelmethode
user-profile-setup-invite-title = Uitnodigingsmail sturen
user-profile-setup-invite-body = Gebruiker ontvangt een e-mail met een beveiligde link om zelf een wachtwoord in te stellen
user-profile-setup-password-title = Wachtwoord handmatig instellen
user-profile-setup-password-body = Maak nu een wachtwoord voor de gebruiker en deel het veilig met hen
user-profile-field-password = Wachtwoord
user-profile-field-password-placeholder = Minimaal 8 tekens
user-profile-field-confirm-password = Bevestig wachtwoord
user-profile-field-confirm-password-placeholder = Voer wachtwoord opnieuw in
user-profile-passwords-match = Wachtwoorden komen overeen
user-profile-passwords-no-match = Wachtwoorden komen niet overeen
user-profile-required-note = Verplichte velden
user-profile-action-cancel = Annuleren
user-profile-action-create = Gebruiker aanmaken
user-profile-action-creating = Aanmaken...
user-profile-devices-title = Apparaten
user-profile-devices-empty = Geen apparaten
user-profile-device-manufacturer-unknown = Onbekend
user-profile-device-last-updated = Laatst bijgewerkt { $when }
user-profile-groups-title = Groepen
user-profile-not-found = Gebruiker niet gevonden
user-profile-error-no-create-permission = Je hebt geen toestemming om gebruikers aan te maken
user-profile-error-missing-id = Gebruikers-ID ontbreekt
user-profile-error-password-too-short = Wachtwoord moet minimaal 8 tekens lang zijn
user-profile-error-passwords-mismatch = Wachtwoorden komen niet overeen
user-profile-error-created-no-uuid = Gebruiker aangemaakt, maar navigatie mislukt. Ga naar de gebruikerslijst.
user-profile-error-save-generic = Opslaan van gebruiker mislukt. Probeer het opnieuw.
user-profile-error-load = Laden van gebruikersprofiel mislukt
user-profile-relative-just-now = zojuist
user-profile-relative-minutes-ago = { $count ->
    [one] { $count } minuut geleden
   *[other] { $count } minuten geleden
   }
user-profile-relative-hours-ago = { $count ->
    [one] { $count } uur geleden
   *[other] { $count } uur geleden
   }
user-profile-relative-days-ago = { $count ->
    [one] { $count } dag geleden
   *[other] { $count } dagen geleden
   }

# Groups management (GroupsManagementView): list, search/sort,
# create modal, delete confirm, and member/device/group count chips.
groups-mgmt-title = Groepen
groups-mgmt-subtitle = Gebruikersgroepen en lidmaatschappen beheren
groups-mgmt-action-new = Nieuwe groep
groups-mgmt-action-new-short = Nieuw
groups-mgmt-loading = Groepen laden...
groups-mgmt-search-placeholder = Groepen zoeken...
groups-mgmt-sort-name = Naam
groups-mgmt-sort-members = Leden
groups-mgmt-sort-devices = Apparaten
groups-mgmt-sort-created = Toegevoegd op
groups-mgmt-sort-ascending = Oplopend
groups-mgmt-sort-descending = Aflopend
groups-mgmt-chip-members = { $count ->
    [one] { $count } lid
   *[other] { $count } leden
   }
groups-mgmt-chip-devices = { $count ->
    [one] { $count } apparaat
   *[other] { $count } apparaten
   }
groups-mgmt-chip-groups = { $count ->
    [one] { $count } groep
   *[other] { $count } groepen
   }
groups-mgmt-action-open-full-page = Volledige pagina openen
groups-mgmt-action-delete = Groep verwijderen
groups-mgmt-no-results = Geen groepen gevonden voor "{ $query }"
groups-mgmt-empty-action = Groep maken
groups-mgmt-modal-create-title = Groep maken
groups-mgmt-field-name = Naam
groups-mgmt-field-name-placeholder = Voer een groepsnaam in
groups-mgmt-field-description = Beschrijving
groups-mgmt-field-description-placeholder = Optionele beschrijving
groups-mgmt-field-color = Kleur
groups-mgmt-action-cancel = Annuleren
groups-mgmt-action-create = Groep maken
groups-mgmt-modal-delete-title = Groep verwijderen
groups-mgmt-delete-confirm-body = Weet je zeker dat je de groep <strong class="text-primary">{ $name }</strong> wilt verwijderen? Hiermee worden alle ledenkoppelingen verwijderd, maar de gebruikers zelf blijven bestaan.
groups-mgmt-action-delete-confirm = Groep verwijderen
groups-mgmt-error-name-required = Groepsnaam is verplicht
groups-mgmt-error-load = Groepen laden mislukt
groups-mgmt-error-create = Groep maken mislukt
groups-mgmt-error-delete = Groep verwijderen mislukt
groups-mgmt-success-created = Groep succesvol aangemaakt
groups-mgmt-success-deleted = Groep succesvol verwijderd

# Group detail (GroupDetailView): per-group page showing
# sync status, members, devices, and creation metadata.
group-detail-error-invalid-id = Ongeldig groeps-ID
group-detail-error-load = Groepsgegevens laden mislukt
group-detail-sync-source-microsoft = Microsoft Entra ID
group-detail-type-security = Beveiliging
group-detail-type-mail-enabled = E-mail ingeschakeld
group-detail-type-standard = Standaard
group-detail-synced-from = Gesynchroniseerd vanuit { $source }
group-detail-action-configure = Configureren
group-detail-section-information = Groepsinformatie
group-detail-field-type = Type
group-detail-field-sync-source = Synchronisatiebron
group-detail-field-last-synced = Laatst gesynchroniseerd
group-detail-field-created = Aangemaakt
group-detail-field-updated = Bijgewerkt
group-detail-section-members = Leden
group-detail-section-devices = Apparaten
group-detail-no-members = Geen leden
group-detail-no-devices = Geen apparaten
group-detail-unknown-device = Onbekend apparaat
group-detail-not-found = Groep niet gevonden

# Devices list (DevicesListView): paginated table with warranty
# filter, sortable columns, bulk delete, and mobile row layout.
devices-list-search-placeholder = Apparaten zoeken...
devices-list-item-label = apparaat
devices-list-filter-warranty-active = Actief
devices-list-filter-warranty-warning = Waarschuwing
devices-list-filter-warranty-expired = Verlopen
devices-list-filter-warranty-unknown = Onbekend
devices-list-filter-warranty-all = Alle garanties
devices-list-column-device = Apparaat
devices-list-column-serial = Serienummer
devices-list-column-hostname = Hostnaam
devices-list-column-model = Model
devices-list-column-user = Gebruiker
devices-list-column-warranty = Garantie
devices-list-add-action = Apparaat toevoegen
devices-list-unassigned = Niet toegewezen
devices-list-warranty-unknown = Onbekend
devices-list-bulk-delete = Verwijderen
devices-list-bulk-delete-count = { $count } verwijderen
devices-list-bulk-action-error = Apparaten verwijderen mislukt. Probeer het opnieuw.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
device-detail-back-to-ticket = Terug naar ticket #{ $id }
device-detail-back-to-devices = Terug
device-detail-readonly = Alleen-lezen
device-detail-delete-item-name = Apparaat
device-detail-error-invalid-id = Ongeldig apparaat-ID
device-detail-error-load = Apparaatgegevens laden mislukt
device-detail-error-create = Apparaat aanmaken mislukt. Probeer het opnieuw.
device-detail-error-delete = Apparaat verwijderen mislukt. Probeer het opnieuw.
device-detail-error-unmanage = Beheer opheffen mislukt. Probeer het opnieuw.
device-detail-section-details = Apparaatgegevens
device-detail-field-name = Naam
device-detail-field-name-placeholder-create = Voer apparaatnaam in
device-detail-field-name-placeholder-edit = Voer naam in...
device-detail-field-hostname = Hostnaam
device-detail-field-hostname-placeholder-create = Voer hostnaam in
device-detail-field-hostname-placeholder-edit = Voer hostnaam in...
device-detail-field-serial = Serienummer
device-detail-field-serial-placeholder-create = Voer serienummer in
device-detail-field-serial-placeholder-edit = Voer serienummer in...
device-detail-field-manufacturer = Fabrikant
device-detail-field-manufacturer-placeholder-create = bijv. Dell, HP, Apple
device-detail-field-manufacturer-placeholder-edit = Voer fabrikant in...
device-detail-field-model = Model
device-detail-field-model-placeholder-create = Voer apparaatmodel in
device-detail-field-model-placeholder-edit = Voer model in...
device-detail-field-warranty-status = Garantiestatus
device-detail-field-warranty-start = Garantie begin
device-detail-field-warranty-end = Garantie einde
device-detail-field-purchase-date = Aankoopdatum
device-detail-field-asset-tag = Inventarisnummer
device-detail-field-asset-tag-placeholder-create = Voer inventarisnummer in
device-detail-field-asset-tag-placeholder-edit = Voer inventarisnummer in...
device-detail-warranty-active = Actief
device-detail-warranty-warning = Waarschuwing
device-detail-warranty-expired = Verlopen
device-detail-warranty-unknown = Onbekend
device-detail-section-primary-user = Primaire gebruiker
device-detail-no-user-assigned = Geen gebruiker toegewezen aan dit apparaat
device-detail-action-assign-user = Gebruiker toewijzen
device-detail-action-change-user = Gebruiker wijzigen
device-detail-section-device-information = Apparaatinformatie
device-detail-field-device-id = Apparaat-ID
device-detail-field-created = Aangemaakt
device-detail-field-last-updated = Laatst bijgewerkt
device-detail-manually-managed = Handmatig beheerd
device-detail-manually-managed-description = Dit apparaat is aangemaakt en wordt handmatig beheerd in Nosdesk
device-detail-section-microsoft-integration = Microsoft-integratie
device-detail-field-last-intune-check-in = Laatste Intune-check-in
device-detail-action-view-in-intune = Bekijken in Intune
device-detail-action-view-in-entra = Bekijken in Entra
device-detail-action-unmanage = Beheer opheffen via Intune/Entra
device-detail-action-unmanage-processing = Verwerken...
device-detail-action-unmanage-title = Verwijderen uit Microsoft Intune/Entra-beheer
device-detail-unmanage-conversion-note = Het apparaat wordt omgezet naar handmatig beheer
device-detail-tech-details-show = Technische details tonen
device-detail-tech-details-hide = Technische details verbergen
device-detail-field-intune-id = Intune-ID
device-detail-field-entra-id = Entra-ID
device-detail-not-managed-by-intune = Dit apparaat wordt niet beheerd door Microsoft Intune
device-detail-action-cancel = Annuleren
device-detail-action-create = Apparaat aanmaken
device-detail-action-create-processing = Aanmaken...
device-detail-not-found = Apparaat niet gevonden
device-detail-unmanage-modal-title = Beheer apparaat opheffen
device-detail-unmanage-heading = Beheer via Microsoft opheffen
device-detail-unmanage-confirm-body = Weet je zeker dat je het beheer van <strong class="text-primary">{ $name }</strong> via Microsoft Intune/Entra wilt opheffen?
device-detail-unmanage-confirm-note = Het apparaat wordt omgezet naar handmatig beheer. Je kunt alle velden bewerken, maar het apparaat synchroniseert niet meer met Microsoft.
device-detail-unmanage-action-confirm = Beheer opheffen

# Projects list (ProjectsView): workspace-wide grid of projects
# rendered from the sync engine pool, with status pills and a
# short description per card.
projects-list-heading = Projecten
projects-list-subheading = Voorvertoning sync-engine (projects_v2-vlag).
projects-list-no-description = Geen beschrijving

# Project detail (ProjectDetailView): per-project kanban board
# with a header, status pill, ticket count, and a Group-by
# control on the kanban toolbar.
project-detail-loading-name = Laden…
project-detail-ticket-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
project-detail-group-by-label = Groeperen op
project-detail-group-by-status = Alleen status
project-detail-group-by-assignee = Status x Toegewezene
project-detail-group-by-priority = Status x Prioriteit
project-detail-loading = Project laden…

# Project Gantt (ProjectGanttView): per-project Gantt timeline
# with a header summary of ticket and dependency-link counts.
project-gantt-fallback-name = Project
project-gantt-summary = { $tickets ->
    [one] { $tickets } ticket
   *[other] { $tickets } tickets
   } · { $links ->
    [one] { $links } koppeling
   *[other] { $links } koppelingen
   }

# Project cycles (ProjectCyclesView): full-page cycles surface
# with active-cycle burndown, create form, and a list of every
# cycle for the project (planned / active / completed).
project-cycles-fallback-name = Project
project-cycles-count = { $count ->
    [one] { $count } cyclus
   *[other] { $count } cycli
   }
project-cycles-new-button = Nieuwe cyclus
project-cycles-cancel-button = Annuleren
project-cycles-date-missing = —
project-cycles-confirm-complete = Deze cyclus afronden? De momentopname wordt dan bevroren.
project-cycles-confirm-archive = Deze cyclus archiveren?
project-cycles-create-title = Nieuwe cyclus
project-cycles-field-name = Naam
project-cycles-field-start = Start
project-cycles-field-end = Einde
project-cycles-name-placeholder = bijv. Sprint 14
project-cycles-create-submit = Aanmaken
project-cycles-all-title = Alle cycli
project-cycles-empty-prefix = Nog geen cycli. Klik op
project-cycles-empty-cta = Nieuwe cyclus
project-cycles-empty-suffix = om een iteratie te starten.
project-cycles-state-planned = gepland
project-cycles-state-active = actief
project-cycles-state-completed = afgerond
project-cycles-action-promote = Activeren
project-cycles-action-complete = Afronden
project-cycles-action-archive = Archiveren

# Workspace cycles (WorkspaceCyclesView): cross-project overview
# of in-flight iterations, grouped by project, with a toggle to
# pull completed cycles back into view.
workspace-cycles-heading = Cycli
workspace-cycles-subheading = Lopende iteraties over alle projecten
workspace-cycles-show-completed = Afgeronde tonen
workspace-cycles-loading = Cycli laden…
workspace-cycles-error-fallback = Cycli konden niet worden geladen
workspace-cycles-empty-title = Nog geen cycli.
workspace-cycles-empty-hint = Open een project en start er een vanuit het Cycli-paneel.
workspace-cycles-group-count = { $count ->
    [one] { $count } cyclus
   *[other] { $count } cycli
   }
workspace-cycles-project-fallback = Project #{ $id }
workspace-cycles-date-missing = —
workspace-cycles-state-planned = gepland
workspace-cycles-state-completed = afgerond

# Cycle detail (CycleDetailView): Scrum board scoped to one
# cycle, with a burndown pinned above the kanban toolbar.
cycle-detail-back = ‹ Cycli
cycle-detail-loading-name = Laden…
cycle-detail-loading = Cyclus laden…
cycle-detail-error-fallback = Cyclus kon niet worden geladen
cycle-detail-summary = { $state } · { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
cycle-detail-group-by-label = Groeperen op
cycle-detail-group-by-status = Alleen status
cycle-detail-group-by-assignee = Status x Toegewezene
cycle-detail-group-by-priority = Status x Prioriteit
cycle-detail-state-planned = Gepland
cycle-detail-state-active = Actief
cycle-detail-state-completed = Afgerond

