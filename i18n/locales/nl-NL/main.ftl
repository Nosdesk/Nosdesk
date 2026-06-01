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
notif-asset-low-stock = [{ $app }] Low stock: { $title }
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
empty-asset-search-prompt-title = Activa zoeken
empty-asset-search-prompt-description = Begin met typen om activa te zoeken op naam, serienummer of gebruiker
empty-asset-search-title = Geen activa gevonden
empty-asset-search-description = Probeer uw zoekopdracht aan te passen
empty-users-default-title = Geen gebruikers gevonden
empty-users-default-description = Nodig gebruikers uit om te beginnen
empty-users-search-title = Geen gebruikers gevonden
empty-users-search-description = Probeer uw zoekopdracht aan te passen
empty-assets-default-title = Geen activa gevonden
empty-assets-default-description = Voeg je eerste activum toe om te beginnen
empty-assets-search-title = Geen activa komen overeen met je zoekopdracht
empty-assets-search-description = Probeer uw zoekopdracht of filters aan te passen
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
nav-assets = Activa
nav-asset-planner = Activaplanner

# Tab strip across the top of the asset section. Used in place
# of duplicate sidebar entries for inventory list + planner.
asset-tabs-inventory = Inventory
asset-tabs-planner = Planner
nav-users = Gebruikers
nav-documentation = Documentatie
nav-inbox = Postvak
nav-collapse = Inklappen
nav-search = Zoeken
nav-more = Meer
nav-toggle-sidebar = Zijbalk wisselen
nav-secondary = Secundaire navigatie
# TODO native-review nl-NL for the bottom-bar pin keys below.
nav-pins-edit = Bewerken
nav-pins-done = Klaar
nav-pins-reset = Opnieuw instellen
nav-pins-edit-hint = { $remaining ->
    [one] Tik op sterren om tot { $max } tegels te kiezen ({ $remaining } plek over)
   *[other] Tik op sterren om tot { $max } tegels te kiezen ({ $remaining } plekken over)
  }
nav-pins-pin = { $name } vastpinnen aan de onderbalk
nav-pins-unpin = { $name } losmaken van de onderbalk
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
password-reset-page-title = Wachtwoord opnieuw instellen
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
admin-workflow-states-archive-confirm-title = Status archiveren?
admin-workflow-states-archive-confirm-label = Archiveren
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
admin-workflow-states-sla-paused = Pauzeert SLA
admin-workflow-states-sla-running = Laat SLA lopen
admin-workflow-states-sla-paused-title = Tickets in deze status pauzeren de SLA-klok. Klik om de klok te laten lopen.
admin-workflow-states-sla-running-title = Tickets in deze status laten de SLA-klok lopen. Klik om te pauzeren.
admin-workflow-states-sla-now-paused-flash = { $name } pauzeert nu de SLA-klok
admin-workflow-states-sla-now-running-flash = { $name } laat nu de SLA-klok lopen

# DRAFT (needs native-review pass): asset-kinds registry.
admin-asset-kinds-title = Activatypen
admin-asset-kinds-description = Definieer de typen activa die u bijhoudt. Elk type heeft een slug (intern gebruikt), een label en een attribuutschema in de JSON Schema-subset die beschrijft welke velden worden verzameld wanneer een activum van dat type wordt aangemaakt.
admin-asset-kinds-builtin-heading = Ingebouwde typen
admin-asset-kinds-builtin-description = Deze typen worden met Nosdesk meegeleverd. U kunt label, beschrijving en attribuutschema bewerken; de slug blijft vast zodat bestaande activa blijven werken.
admin-asset-kinds-builtin-tag = ingebouwd
admin-asset-kinds-custom-heading = Aangepaste typen
admin-asset-kinds-custom-description = Door beheerders gedefinieerde typen voor alles wat u wilt bijhouden (materialen, voertuigen, licenties). De slug is na aanmaken onveranderlijk.
admin-asset-kinds-custom-empty = Nog geen aangepaste typen. Gebruik het onderstaande formulier om er een toe te voegen.
admin-asset-kinds-create-heading = Een nieuw type aanmaken
admin-asset-kinds-create-button = Type aanmaken
admin-asset-kinds-edit = Bewerken
admin-asset-kinds-edit-schema = Bewerken
admin-asset-kinds-delete = Verwijderen
admin-asset-kinds-save = Opslaan
admin-asset-kinds-cancel = Annuleren
admin-asset-kinds-field-slug = Slug
admin-asset-kinds-field-slug-placeholder = bijv. loodgieter_onderdeel
admin-asset-kinds-field-label = Label
admin-asset-kinds-field-description = Beschrijving
admin-asset-kinds-field-icon = Pictogramnaam
admin-asset-kinds-field-sort-order = Sorteervolgorde
admin-asset-kinds-field-category = Categorie
admin-asset-kinds-field-attribute-schema = Attribuutschema (JSON)
admin-asset-kinds-category-it = IT-apparaat
admin-asset-kinds-category-logical = Logisch (licentie, abonnement)
admin-asset-kinds-category-physical = Fysiek (voertuig, uitrusting)
admin-asset-kinds-category-bulk = Bulk (gemeten op aantal en eenheid)
admin-asset-kinds-category-generic = Algemeen
admin-asset-kinds-saved = Opgeslagen

# Schema-conflict surface when an attribute_schema change would invalidate existing rows.
admin-asset-kinds-conflict-heading = { $count } existing asset(s) would no longer validate against this schema:
admin-asset-kinds-conflict-help = Fix the listed assets first, or click Force save to apply the schema change anyway. Force-saved assets stay in the database with their old attributes; the asset detail page will flag them.
admin-asset-kinds-force-save = Force save
admin-asset-kinds-created = Type aangemaakt
admin-asset-kinds-deleted = { $label } verwijderd
admin-asset-kinds-error-load = Activatypen laden mislukt
admin-asset-kinds-error-save = Type opslaan mislukt
admin-asset-kinds-error-create = Type aanmaken mislukt
admin-asset-kinds-error-delete = Type verwijderen mislukt
admin-asset-kinds-error-slug-required = Slug is verplicht
admin-asset-kinds-error-label-required = Label is verplicht
admin-asset-kinds-error-bad-schema-json = Attribuutschema is geen geldige JSON: { $error }
admin-asset-kinds-delete-confirm-title = Activatype verwijderen?
admin-asset-kinds-delete-confirm = "{ $label }" verwijderen? Bestaande activa behouden deze waarde, maar u kunt er geen nieuwe meer aanmaken totdat u het type opnieuw toevoegt.
admin-asset-kinds-new = Nieuw type
admin-asset-kinds-back-label = Terug naar activatypen
admin-asset-kinds-search-placeholder = Zoeken op label, slug of beschrijving...
admin-asset-kinds-loading = Activatypen laden...
admin-asset-kinds-empty-title = Nog geen activatypen
admin-asset-kinds-empty-description = Maak uw eerste activatype aan om te beschrijven wat uw team bijhoudt.
admin-asset-kinds-no-matches-title = Geen overeenkomende typen
admin-asset-kinds-no-matches-description = Niets komt overeen met "{ $query }". Probeer een ander woord.
admin-asset-kinds-updated = Bijgewerkt { $when }
admin-asset-kinds-delete-aria = Type { $label } verwijderen
admin-asset-kinds-delete-confirm-zero = "{ $label }" verwijderen? Geen activa gebruiken dit type op dit moment.
admin-asset-kinds-delete-confirm-with-count = "{ $label }" verwijderen? { $count } bestaande activa verwijzen naar dit type. Ze behouden de slug-waarde, maar u kunt er geen nieuwe meer aanmaken totdat het type opnieuw is toegevoegd.
admin-asset-kinds-builtin-no-delete = Ingebouwde typen kunnen niet worden verwijderd
admin-asset-kinds-create-title = Nieuw activatype
admin-asset-kinds-edit-title = Activatype bewerken
admin-asset-kinds-edit-not-found = Dit activatype is niet gevonden. Mogelijk is het in een ander tabblad verwijderd.
admin-asset-kinds-prettify = JSON opmaken
admin-asset-kinds-field-slug-hint = Kleine letters, cijfers en underscores. Voor intern gebruik; kan niet meer worden gewijzigd als het type bestaat.
admin-asset-kinds-field-slug-locked = Slug is na aanmaken vergrendeld zodat bestaande activarijen blijven herleiden.
admin-asset-kinds-field-icon-hint = Optionele pictogramnaam (bijv. "monitor", "phone"). Wordt getoond in de activakiezer.
admin-asset-kinds-field-attribute-schema-hint = JSON Schema-subset. De Bouwer-weergave is standaard; schakel naar JSON-weergave voor handmatige bewerkingen.
admin-asset-kinds-view-builder = Bouwer tonen
admin-asset-kinds-view-json = JSON tonen

# Getypte attribuutbouwer.
asset-kind-attribute-editor-add = Attribuut toevoegen
asset-kind-attribute-editor-empty-title = Nog geen attributen
asset-kind-attribute-editor-empty-description = Klik op "Attribuut toevoegen" om een veld te beschrijven dat het activaformulier moet verzamelen.
asset-kind-attribute-editor-parse-error = Schema kan niet worden geparsed: { $error }. Schakel naar JSON-weergave om dit direct op te lossen.
asset-kind-attribute-row-move = Positie
asset-kind-attribute-row-move-up = Omhoog verplaatsen
asset-kind-attribute-row-move-down = Omlaag verplaatsen
asset-kind-attribute-row-remove = Attribuut verwijderen
asset-kind-attribute-row-name = Naam
asset-kind-attribute-row-name-placeholder = bijv. serial_number
asset-kind-attribute-row-name-hint = Kleine letters, cijfers en underscores. Wordt gebruikt als JSON-sleutel.
asset-kind-attribute-row-name-invalid = Moet bestaan uit kleine letters, cijfers of underscores, en beginnen met een letter.
asset-kind-attribute-row-kind = Type
asset-kind-attribute-row-required = Verplicht
asset-kind-attribute-row-description = Beschrijving
asset-kind-attribute-row-description-placeholder = Optionele hulptekst onder het veld
asset-kind-attribute-row-raw-warning = Niet-herkende eigenschapsvorm. Bewerk dit attribuut via JSON-weergave; de bouwer behoudt het bij opslaan.
asset-kind-attribute-row-max-length = Max lengte
asset-kind-attribute-row-pattern = Patroon (regex)
asset-kind-attribute-row-pattern-hint = Optioneel. POSIX regex; bijv. ^[A-Z0-9-]+$ voor hoofdletters + cijfers.
asset-kind-attribute-row-minimum = Minimum
asset-kind-attribute-row-maximum = Maximum
asset-kind-attribute-row-enum-values = Toegestane waarden
asset-kind-attribute-row-enum-remove = Waarde { $value } verwijderen
asset-kind-attribute-row-enum-add-placeholder = Typ een waarde, druk op Enter
asset-kind-attribute-row-enum-empty = Voeg minstens één toegestane waarde toe.
asset-kind-attribute-kind-text = Tekst
asset-kind-attribute-kind-email = E-mail
asset-kind-attribute-kind-url = URL
asset-kind-attribute-kind-number = Getal (geheel)
asset-kind-attribute-kind-decimal = Decimaal
asset-kind-attribute-kind-boolean = Ja / Nee
asset-kind-attribute-kind-date = Datum
asset-kind-attribute-kind-datetime = Datum en tijd
asset-kind-attribute-kind-select = Eén keuze
asset-kind-attribute-kind-multi_select = Meerdere keuzes
asset-kind-attribute-kind-user = Gebruikersverwijzing
asset-kind-attribute-kind-asset = Activaverwijzing
asset-kind-attribute-kind-raw = Aangepast (alleen-lezen)
asset-kind-attribute-user-loading = Gebruikers laden...
asset-kind-attribute-user-none = Geen gebruiker geselecteerd
asset-kind-attribute-user-load-error = Kan gebruikers niet laden
asset-kind-attribute-row-asset-scope = Beperken tot activatype
asset-kind-attribute-row-asset-scope-any = Elk type
asset-kind-attribute-row-asset-scope-hint = Beperkt de invoerkiezer tot activa van het gekozen type. Laat op "Elk type" om verwijzingen naar elk actief toe te staan.
asset-kind-attribute-asset-loading = Activa laden...
asset-kind-attribute-asset-none = Geen actief geselecteerd
asset-kind-attribute-asset-load-error = Kan activa niet laden
asset-kind-attribute-asset-empty-for-scope = Nog geen activa van type "{ $kind }".

admin-nav-asset-kinds-title = Activatypen
admin-nav-asset-kinds-description = Definieer de activatypen die u bijhoudt en de attributen per type

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
admin-nav-canned-responses-title = Standaardantwoorden
admin-nav-canned-responses-description = Herbruikbare antwoordsjablonen met variabelen
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
admin-nav-workspaces-title = Workspaces
admin-nav-workspaces-description = Create, archive, and manage tenant workspaces and their members.
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
admin-search-mgmt-description = Beheer de zoekindex voor tickets, documentatie, activa en gebruikers.
admin-search-mgmt-stats-title = Indexstatistieken
admin-search-mgmt-refresh = Vernieuwen
admin-search-mgmt-stats-loading = Zoekindexstatistieken laden
admin-search-mgmt-total-documents = Totaal documenten
admin-search-mgmt-index-size = Indexgrootte
admin-search-mgmt-status = Status
admin-search-mgmt-status-rebuilding = Opnieuw opbouwen
admin-search-mgmt-status-ready = Gereed
admin-search-mgmt-entity-types = Entiteitstypen
admin-search-mgmt-stats-error = Kon zoekindexstatistieken niet ophalen
admin-search-mgmt-rebuild-title = Zoekindex opnieuw opbouwen
admin-search-mgmt-rebuild-description = Bouwt de volledige zoekindex opnieuw op vanuit de database. Herindexeert alle tickets, opmerkingen, documentatiepagina's, bijlagen, activa en gebruikers. Gebruik dit als zoekresultaten ontbreken of verouderd zijn.
admin-search-mgmt-rebuild = Index opnieuw opbouwen
admin-search-mgmt-rebuilding = Opnieuw opbouwen...
admin-search-mgmt-rebuild-success = Index succesvol opnieuw opgebouwd
admin-search-mgmt-rebuild-failed = Opnieuw opbouwen mislukt
admin-search-mgmt-rebuild-stat-tickets = Tickets:
admin-search-mgmt-rebuild-stat-comments = Reacties:
admin-search-mgmt-rebuild-stat-docs = Documenten:
admin-search-mgmt-rebuild-stat-attachments = Bijlagen:
admin-search-mgmt-rebuild-stat-devices = Activa:
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
admin-guest-default-priority-hint = Wordt toegepast op elke gastindiening. Agenten kunnen achteraf opnieuw triëren.
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
admin-data-import-csv-description = Importeer gegevens uit CSV-bestanden, waaronder activa, gebruikers en andere bronnen
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
admin-api-tokens-revoke-confirm-message = Weet u zeker dat u het token "{ $name }" wilt intrekken?
admin-api-tokens-revoke-warning = Deze actie kan niet ongedaan worden gemaakt. Systemen die dit token gebruiken verliezen toegang.
admin-api-tokens-revoking = Intrekken...

# Admin: Standaardantwoorden (CannedResponsesView). Herbruikbare
# antwoordsjablonen die de samensteller-selector op aanvraag
# invoegt. Werkruimtebrede gedeelde bibliotheek; schrijven alleen
# voor beheerders.
admin-canned-responses-title = Standaardantwoorden
admin-canned-responses-description = Herbruikbare antwoordsjablonen die agenten met één klik in de ticketsamensteller kunnen invoegen. {"{{"}variable{"}}"}-tokens worden bij het invoegen vervangen.
admin-canned-responses-loading = Sjablonen laden...
admin-canned-responses-create = Nieuw sjabloon
admin-canned-responses-create-title = Nieuw standaardantwoord
admin-canned-responses-edit-title = Standaardantwoord bewerken
admin-canned-responses-create-submit = Aanmaken
admin-canned-responses-save = Wijzigingen opslaan
admin-canned-responses-cancel = Annuleren
admin-canned-responses-search-placeholder = Zoeken op titel of inhoud...
admin-canned-responses-search-aria = Standaardantwoorden zoeken
admin-canned-responses-column-name = Naam
admin-canned-responses-column-updated = Bijgewerkt
admin-canned-responses-column-inserts = Invoegingen
admin-canned-responses-column-inserts-title = Aantal invoegingen in de laatste 30 dagen
admin-canned-responses-delete-title = Sjabloon verwijderen
admin-canned-responses-delete-aria = Sjabloon { $name } verwijderen
admin-canned-responses-delete-confirm-title = Standaardantwoord verwijderen
admin-canned-responses-delete-confirm-message = Definitief "{ $name }" verwijderen? Agenten zien het daarna niet meer in de samensteller.
admin-canned-responses-delete-confirm-button = Verwijderen
admin-canned-responses-empty-title = Nog geen standaardantwoorden
admin-canned-responses-empty-description = Maak uw eerste antwoordsjabloon aan, dan kunnen agenten het met één klik invoegen vanuit de samensteller.
admin-canned-responses-no-matches-title = Geen overeenkomende sjablonen
admin-canned-responses-no-matches-description = Niets komt overeen met "{ $query }". Probeer een ander woord.
admin-canned-responses-field-title = Titel
admin-canned-responses-field-title-placeholder = bv. Wachtwoord opnieuw instellen
admin-canned-responses-field-body = Inhoud
admin-canned-responses-field-body-placeholder = Beste {"{{"}customer_name{"}}"}, ...
admin-canned-responses-field-body-hint = Ondersteunde variabelen: { $variables }
admin-canned-responses-warn-unknown-variables = Onbekende variabelen: { $names }. Ze verschijnen letterlijk in klantenantwoorden; corrigeer of verwijder ze.
admin-canned-responses-error-load = Kan standaardantwoorden niet laden
admin-canned-responses-error-save = Opslaan mislukt
admin-canned-responses-error-delete = Verwijderen mislukt
admin-canned-responses-error-title-required = Titel is verplicht
admin-canned-responses-error-body-required = Inhoud is verplicht
admin-canned-responses-error-unknown-variables = Onbekende variabelen: { $names }. Verwijder of corrigeer ze voor het opslaan.
admin-canned-responses-success-created = Standaardantwoord aangemaakt
admin-canned-responses-success-updated = Standaardantwoord opgeslagen
admin-canned-responses-success-deleted = Standaardantwoord verwijderd
admin-canned-responses-browse-starters = Sjablonen bekijken
admin-canned-responses-editor-insert-label = Invoegen:
admin-canned-responses-edit-back-label = Terug naar standaardantwoorden
admin-canned-responses-editor-variable-aria = Variabele: { $name }
admin-canned-responses-editor-insert-variable-aria = Variabele { $name } invoegen
admin-canned-responses-edit-not-found = Dit standaardantwoord is niet gevonden. Mogelijk is het in een ander tabblad verwijderd.
admin-canned-responses-preview-heading = Voorbeeld
admin-canned-responses-preview-empty = De inhoud is leeg. Begin met typen in de editor om het voorbeeld te zien.
admin-canned-responses-preview-hint = Weergegeven met voorbeeldwaarden. Echte tickets vervangen de waarden die de selector op het moment van invoegen heeft.
admin-canned-responses-starters-title = Start vanuit een sjabloon
admin-canned-responses-starters-description = Kies een startsjabloon als uitgangspunt. U kunt alles aanpassen voor het opslaan.
admin-canned-responses-starters-loading = Sjablonen laden...
admin-canned-responses-starters-error-load = Kan startsjablonen niet laden
admin-canned-responses-starters-use = Gebruiken

# Beheer: SLA.
admin-sla-title = SLA
admin-sla-no-calendars-hint = Geen kalenders. Voeg er hieronder een toe — elk SLA-beleid heeft een kalender nodig om doelen te berekenen.
admin-sla-no-policies-hint = Geen SLA-beleid. Voeg er hieronder een toe — zonder beleid hebben tickets geen SLA-pil.
admin-sla-description = Werkkalenders en SLA-beleid voeden de SLA-pil per ticket. Nosdesk levert standaard een kalender ma–vr 9–17 UTC en een beleid van 4 u reactie / 24 u oplossing. Bewerk ze hieronder of voeg nieuwe items toe voor specifieke categorieën of prioriteiten.
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
admin-sla-col-targets = Doelen
admin-sla-col-matches = Overeenkomsten
admin-sla-matches-none = geen
admin-sla-matches-total = { $count } overeen.
admin-sla-matches-at-risk = { $count } risico
admin-sla-matches-breached = { $count } overschr.
admin-sla-matches-at-risk-title = Tickets binnen 25% van het reactiedoel.
admin-sla-matches-breached-title = Tickets die het reactiedoel hebben overschreden.
admin-sla-col-calendar = Kalender
admin-sla-default-badge = Standaard
admin-sla-set-default = Standaard instellen
admin-sla-delete = Verwijderen
admin-sla-edit = Bewerken
admin-sla-save = Opslaan
admin-sla-cancel = Annuleren
admin-sla-delete-confirm-title = Verwijderen?
admin-sla-calendar-delete-confirm = Deze kalender verwijderen? Beleid dat ernaar verwijst heeft een nieuwe kalender nodig.
admin-sla-policy-delete-confirm = Dit beleid verwijderen? Tickets die er momenteel onder vallen verliezen hun SLA-indicator totdat een ander beleid past. Dit kan niet ongedaan worden gemaakt.
admin-sla-new-calendar-heading = Nieuwe kalender
admin-sla-new-policy-heading = Nieuw beleid
admin-sla-new-calendar-button = Nieuwe kalender
admin-sla-new-policy-button = Nieuw beleid
admin-sla-new-calendar-title = Nieuwe werkkalender
admin-sla-new-policy-title = Nieuw SLA-beleid
admin-sla-edit-policy-title = SLA-beleid bewerken
admin-sla-error-save = Opslaan mislukt
admin-sla-form-conditions-heading = Voorwaarden
admin-sla-form-targets-heading = Doelen
admin-sla-field-name = Naam
admin-sla-field-tz = Tijdzone
admin-sla-field-calendar = Kalender
admin-sla-field-response = Reactie (minuten)
admin-sla-field-resolution = Oplossing (minuten)
admin-sla-field-priority = Prioriteitsfilter
admin-sla-field-category = Categoriefilter
admin-sla-field-assignee-group = Filter op toegewezen groep
admin-sla-placeholder-name = EU-support-uren
admin-sla-placeholder-tz = Kies een tijdzone
admin-sla-tz-search-placeholder = Tijdzones zoeken...
admin-sla-tz-no-matches = Geen overeenkomende tijdzones
admin-sla-policy-name-placeholder = Kritieke incidenten
admin-sla-edit-calendar-title = Werkkalender bewerken
admin-sla-field-schedule = Werkuren
admin-sla-schedule-day-mon = Ma
admin-sla-schedule-day-tue = Di
admin-sla-schedule-day-wed = Wo
admin-sla-schedule-day-thu = Do
admin-sla-schedule-day-fri = Vr
admin-sla-schedule-day-sat = Za
admin-sla-schedule-day-sun = Zo
admin-sla-schedule-remove-range-aria = Dit bereik verwijderen
admin-sla-schedule-resize-open-aria = Sleep om de openingstijd te wijzigen
admin-sla-schedule-resize-close-aria = Sleep om de sluitingstijd te wijzigen
admin-sla-schedule-timeline-hint = Klik op een lege baan om uren toe te voegen; sleep de randen van een balk om te schalen, of klik op de balk om een precieze tijd te typen.
admin-sla-schedule-edit-range-aria = Tijdsbereik bewerken
admin-sla-field-holidays = Feestdagen
admin-sla-holidays-empty-hint = Nog geen feestdagen. Voeg hieronder een datum toe om die als niet-werkdag te markeren.
admin-sla-holiday-date = Datum
admin-sla-holiday-label = Label
admin-sla-holiday-placeholder = bv. Feestdag
admin-sla-holiday-add = Toevoegen
admin-sla-holiday-remove-aria = Deze feestdag verwijderen
admin-sla-holiday-annual = Jaarlijks herhalend
admin-sla-holiday-annual-hint = Herhaalt dezelfde MM-DD elk jaar (bv. Kerstmis).
admin-sla-holiday-annual-badge = Jaarlijks
admin-sla-holiday-import-label = Voorinstelling importeren:
admin-sla-holiday-import-placeholder = Land kiezen...
admin-sla-holiday-import-summary = { $country }: { $added } toegevoegd, { $skipped } overgeslagen (al aanwezig)

sla-explain-aria = SLA-uitleg
sla-explain-title = Waarom dit SLA?
sla-explain-error = SLA-uitleg kon niet worden geladen.
sla-explain-no-policy = Geen SLA-beleid komt overeen met dit ticket, dus geen doelen van toepassing.
sla-explain-default-badge = Werkruimtestandaard
sla-explain-no-filters = Overeenkomst als werkruimtestandaard (geen filters ingesteld).
sla-explain-filter-priority = Prioriteit is { $value }
sla-explain-filter-category = Categorie is { $name }
sla-explain-filter-group = Toegewezene zit in { $name }
sla-explain-calendar-label = Kalender
sla-explain-targets-label = Doelen
sla-explain-targets = { $response } reactie · { $resolution } oplossing
sla-explain-state-label = Status
sla-explain-state-running = Klok loopt ({ $state })
sla-explain-state-paused = Klok gepauzeerd ({ $state })
sla-explain-fmt-minutes = { $n }m
sla-explain-fmt-hours = { $n }u
sla-explain-fmt-days = { $n }d

ticket-detail-sla-explain-aria = Toon waarom dit SLA is gekozen

time-picker-hours-aria = Uren
time-picker-minutes-aria = Minuten
date-picker-prev-month-aria = Vorige maand
date-picker-next-month-aria = Volgende maand
admin-sla-priority-any = Alle
admin-sla-category-any = Alle
admin-sla-assignee-group-any = Alle
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
admin-branding-signature-default-label = Standaard e-mailhandtekening
admin-branding-signature-default-placeholder = Met vriendelijke groet, het Supportteam
admin-branding-signature-default-hint = Gebruikt voor medewerkers zonder persoonlijke handtekening. Laat leeg om antwoorden zonder handtekening te versturen.
admin-branding-signature-default-variables-hint = Variabelen (per antwoord ingevuld):
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
admin-channels-email-description = Vraag een supportmailbox op via IMAP en zet inkomende berichten om in tickets. Reacties van agenten worden via dezelfde thread teruggestuurd.
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
admin-channels-email-test-dirty-hint = Sla wijzigingen eerst op om ze te testen.
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
admin-channels-email-error-load = Kon e-mailkanaal niet laden
admin-channels-email-success-update = Kanaal bijgewerkt
admin-channels-email-success-create = Kanaal aangemaakt
admin-channels-email-success-password-removed = Wachtwoord verwijderd
admin-channels-email-success-delete = Kanaal verwijderd
admin-channels-email-auto-ack-heading = Automatische ontvangstbevestiging
admin-channels-email-auto-ack-subtitle = Wanneer een nieuwe e-mail een ticket opent, stuurt u een kort "we hebben uw bericht ontvangen"-antwoord zodat de klant weet dat het is aangekomen.
admin-channels-email-auto-ack-toggle-label = Ontvangstbevestiging verzenden
admin-channels-email-auto-ack-toggle-description = Schakel uit als uw team binnen enkele minuten handmatig wil reageren.
admin-channels-email-auto-ack-template-label = Aangepast sjabloon
admin-channels-email-auto-ack-template-placeholder = Hallo {"{{"}customer_name{"}}"}, we hebben uw bericht ontvangen en komen spoedig bij u terug. (ref #{"{{"}ticket_id{"}}"})
admin-channels-email-auto-ack-template-hint = Laat leeg om de gelokaliseerde standaard te gebruiken. Alleen platte tekst.
admin-channels-email-auto-ack-variables-hint = Variabelen (per ticket ingevuld):
admin-channels-email-auto-ack-saving = Opslaan…
admin-channels-email-auto-ack-save = Ontvangstbevestiging opslaan
admin-channels-email-auto-ack-success-saved = Ontvangstbevestiging bijgewerkt

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
admin-webhooks-category-assets = Apparaten
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
admin-webhooks-event-asset-linked = Apparaat gekoppeld
admin-webhooks-event-asset-unlinked = Apparaat ontkoppeld
admin-webhooks-event-asset-updated = Apparaat bijgewerkt
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
user-mgmt-filter-name-label = Name
user-mgmt-filter-role-label = Role
user-mgmt-filter-deleted-label = Deleted
user-mgmt-filter-deleted-on = Show deleted
user-mgmt-grouping-role = Role
user-mgmt-grouping-status = Status
user-mgmt-grouping-status-active = Active
user-mgmt-grouping-status-deleted = Deleted
user-mgmt-grouping-joined = Joined
user-mgmt-grouping-joined-this-month = Last 30 days
user-mgmt-grouping-joined-this-year = This year
user-mgmt-grouping-joined-older = Earlier
user-mgmt-role-admin = Beheerder
user-mgmt-role-technician = Agent
user-mgmt-role-user = Gebruiker
user-mgmt-column-user = Gebruiker
user-mgmt-column-role = Rol
user-mgmt-column-tickets = Tickets
user-mgmt-column-assets = Activa
user-mgmt-column-joined = Lid sinds
user-mgmt-invite-action = Gebruiker uitnodigen
user-mgmt-mobile-tickets = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
   }
user-mgmt-mobile-assets = { $count ->
    [one] { $count } apparaat
   *[other] { $count } apparaten
   }
user-mgmt-bulk-role = Rol
user-mgmt-bulk-delete = Verwijderen
user-mgmt-bulk-delete-count = { $count } verwijderen
user-mgmt-bulk-delete-title = { $count ->
    [one] Gebruiker verwijderen?
   *[other] { $count } gebruikers verwijderen?
}
user-mgmt-bulk-delete-message = { $count ->
    [one] Eén gebruiker verwijderen? Deze kan 30 dagen worden hersteld voordat de gebruiker permanent wordt verwijderd.
   *[other] { $count } gebruikers verwijderen? Deze kunnen 30 dagen worden hersteld voordat ze permanent worden verwijderd.
}
user-mgmt-bulk-action-error = Bulkactie mislukt. Probeer het opnieuw.
# TODO native-review nl-NL for the soft-delete keys below.
user-mgmt-deleted-off = Verwijderde tonen
user-mgmt-deleted-on = Verwijderde verbergen
user-mgmt-deleted-badge = Verwijderd
user-mgmt-deleted-purges-on = Definitief verwijderd op { $date }
user-mgmt-restore = Gebruiker herstellen
user-mgmt-restored = { $name } hersteld
user-mgmt-restore-error = Kon die gebruiker niet herstellen.
user-mgmt-purge-now = Permanent verwijderen
user-mgmt-purged = { $name } permanent verwijderd
user-mgmt-purge-error = Kon die gebruiker niet permanent verwijderen.
user-mgmt-purge-title = Gebruiker permanent verwijderen?
user-mgmt-purge-message = { $name } permanent verwijderen? Dit slaat het herstelvenster van 30 dagen over en kan niet ongedaan gemaakt worden.
user-mgmt-purge-confirm = Permanent verwijderen
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
user-profile-role-technician = Agent
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
user-profile-assets-title = Activa
user-profile-assets-empty = Geen activa
user-profile-asset-manufacturer-unknown = Onbekend
user-profile-asset-last-updated = Laatst bijgewerkt { $when }
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
groups-mgmt-sort-assets = Activa
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
group-detail-section-devices = Activa
group-detail-no-members = Geen leden
group-detail-no-devices = Geen activa
group-detail-unknown-device = Onbekend activum
group-detail-not-found = Groep niet gevonden

# Devices list (DevicesListView): paginated table with warranty
# filter, sortable columns, bulk delete, and mobile row layout.
assets-list-search-placeholder = Activa zoeken...
assets-list-item-label = activum
assets-list-filter-warranty-active = Actief
assets-list-filter-warranty-warning = Waarschuwing
assets-list-filter-warranty-expired = Verlopen
assets-list-filter-warranty-unknown = Onbekend
assets-list-filter-warranty-all = Alle garanties
assets-list-filter-name-label = Name
assets-list-filter-warranty-label = Warranty
assets-list-filter-low-stock-label = Low stock
assets-list-column-device = Activum
assets-list-column-serial = Serienummer
assets-list-column-hostname = Hostnaam
assets-list-column-model = Model
assets-list-column-user = Gebruiker
assets-list-column-warranty = Garantie

assets-list-column-stock = Stock
assets-list-filter-low-stock-all = All stock
assets-list-filter-low-stock-on = Low stock only
assets-list-add-action = Activum toevoegen
assets-list-unassigned = Niet toegewezen
assets-list-warranty-unknown = Onbekend
assets-list-bulk-delete = Verwijderen
assets-list-bulk-delete-count = { $count } verwijderen
assets-list-bulk-delete-title = { $count ->
    [one] Apparaat verwijderen?
   *[other] { $count } apparaten verwijderen?
}
assets-list-bulk-delete-message = { $count ->
    [one] Hiermee wordt één apparaat permanent verwijderd. Deze actie kan niet ongedaan worden gemaakt.
   *[other] Hiermee worden { $count } apparaten permanent verwijderd. Deze actie kan niet ongedaan worden gemaakt.
}
assets-list-bulk-action-error = Verwijderen van activa mislukt. Probeer het opnieuw.

# Device detail (DeviceView): per-device page covering name, hostname,
# hardware identifiers, warranty fields, primary user, Microsoft Intune
# integration, and the unmanage / create flows.
asset-detail-back-to-ticket = Terug naar ticket #{ $id }
asset-detail-back-to-devices = Terug
asset-detail-readonly = Alleen-lezen
asset-detail-delete-item-name = Activum
asset-detail-error-invalid-id = Ongeldige activum-ID
asset-detail-error-load = Activumgegevens konden niet worden geladen
asset-detail-error-create = Aanmaken van activum mislukt. Probeer het opnieuw.
asset-detail-error-delete = Verwijderen van activum mislukt. Probeer het opnieuw.
asset-detail-error-unmanage = Beheer opheffen mislukt. Probeer het opnieuw.
asset-detail-section-details = Activumgegevens
asset-detail-section-kind = Activatype
asset-detail-field-kind = Type
asset-detail-field-name = Naam
asset-detail-field-name-placeholder-create = Voer activumnaam in
asset-detail-field-name-placeholder-edit = Voer naam in...
asset-detail-field-hostname = Hostnaam
asset-detail-field-hostname-placeholder-create = Voer hostnaam in
asset-detail-field-hostname-placeholder-edit = Voer hostnaam in...
asset-detail-field-serial = Serienummer
asset-detail-field-serial-placeholder-create = Voer serienummer in
asset-detail-field-serial-placeholder-edit = Voer serienummer in...
asset-detail-field-manufacturer = Fabrikant
asset-detail-field-manufacturer-placeholder-create = bijv. Dell, HP, Apple
asset-detail-field-manufacturer-placeholder-edit = Voer fabrikant in...
asset-detail-field-model = Model
asset-detail-field-model-placeholder-create = Voer activumtype in
asset-detail-field-model-placeholder-edit = Voer model in...
asset-detail-field-warranty-status = Garantiestatus
asset-detail-field-warranty-start = Garantie begin
asset-detail-field-warranty-end = Garantie einde
asset-detail-field-purchase-date = Aankoopdatum
asset-detail-field-asset-tag = Inventarisnummer
asset-detail-field-asset-tag-placeholder-create = Voer inventarisnummer in
asset-detail-field-asset-tag-placeholder-edit = Voer inventarisnummer in...
asset-detail-warranty-active = Actief
asset-detail-warranty-warning = Waarschuwing
asset-detail-warranty-expired = Verlopen
asset-detail-warranty-unknown = Onbekend
asset-detail-section-primary-user = Primaire gebruiker
asset-detail-no-user-assigned = Geen gebruiker toegewezen aan dit activum
asset-detail-action-assign-user = Gebruiker toewijzen
asset-detail-action-change-user = Gebruiker wijzigen
asset-detail-section-device-information = Apparaatinformatie
asset-detail-field-device-id = Apparaat-ID
asset-detail-field-created = Aangemaakt
asset-detail-field-last-updated = Laatst bijgewerkt
asset-detail-manually-managed = Handmatig beheerd
asset-detail-manually-managed-description = Dit apparaat is aangemaakt en wordt handmatig beheerd in Nosdesk
asset-detail-section-microsoft-integration = Microsoft-integratie
asset-detail-field-last-intune-check-in = Laatste Intune-check-in
asset-detail-action-view-in-intune = Bekijken in Intune
asset-detail-action-view-in-entra = Bekijken in Entra
asset-detail-action-unmanage = Beheer opheffen via Intune/Entra
asset-detail-action-unmanage-processing = Verwerken...
asset-detail-action-unmanage-title = Verwijderen uit Microsoft Intune/Entra-beheer
asset-detail-unmanage-conversion-note = Het apparaat wordt omgezet naar handmatig beheer
asset-detail-tech-details-show = Technische details tonen
asset-detail-tech-details-hide = Technische details verbergen
asset-detail-field-intune-id = Intune-ID
asset-detail-field-entra-id = Entra-ID
asset-detail-not-managed-by-intune = Dit apparaat wordt niet beheerd door Microsoft Intune
asset-detail-action-cancel = Annuleren
asset-detail-action-create = Activum aanmaken
asset-detail-action-create-processing = Aanmaken...
asset-detail-not-found = Activum niet gevonden
asset-detail-unmanage-modal-title = Beheer apparaat opheffen
asset-detail-unmanage-heading = Beheer via Microsoft opheffen
asset-detail-unmanage-confirm-body = Weet je zeker dat je het beheer van <strong class="text-primary">{ $name }</strong> via Microsoft Intune/Entra wilt opheffen?
asset-detail-unmanage-confirm-note = Het apparaat wordt omgezet naar handmatig beheer. Je kunt alle velden bewerken, maar het apparaat synchroniseert niet meer met Microsoft.
asset-detail-unmanage-action-confirm = Beheer opheffen

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
project-cycles-confirm-complete-title = Cyclus afronden?
project-cycles-confirm-archive-title = Cyclus archiveren?
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

# Documentation index (DocumentationIndexView): hub page listing
# recently updated, starred, collections, and status chips.
docs-index-title = Documentatie
docs-index-new-page = Nieuwe pagina
docs-index-recently-updated = Recent bijgewerkt
docs-index-recently-updated-count = Laatste { $count }
docs-index-no-recent-activity = Geen recente activiteit.
docs-index-starred = Favorieten
docs-index-starred-hint = Markeer een pagina als favoriet via het rijmenu voor snelle toegang.
docs-index-browse-all = Alle pagina's bekijken
docs-index-chip-drafts = { $count ->
    [one] { $count } concept
   *[other] { $count } concepten
   }
docs-index-chip-archived = { $count } gearchiveerd
docs-index-chip-trash = { $count } in prullenbak

# Documentation drafts (DocumentationDraftsView): pages not yet
# assigned to a collection.
docs-drafts-title = Concepten
docs-drafts-heading = Concepten
docs-drafts-description = Pagina's die nog niet aan een collectie zijn toegewezen
docs-drafts-back = Terug naar Documentatie
docs-drafts-count = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
   }

# Documentation archived (DocumentationArchivedView): list of
# pages that have been archived, with a restore action.
docs-archived-title = Gearchiveerd
docs-archived-heading = Gearchiveerd
docs-archived-description = Pagina's die zijn gearchiveerd
docs-archived-back = Terug naar Documentatie
docs-archived-count = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
   }
docs-archived-loading = Gearchiveerde pagina's laden
docs-archived-archived-at = Gearchiveerd op { $date }
docs-archived-restore = Herstellen

# Documentation trash (DocumentationTrashView): deleted pages,
# with restore and permanent-delete actions.
docs-trash-title = Prullenbak
docs-trash-heading = Prullenbak
docs-trash-description = Verwijderde pagina's kunnen worden hersteld of permanent worden verwijderd
docs-trash-back = Terug naar Documentatie
docs-trash-count = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
   }
docs-trash-loading = Verwijderde pagina's laden
docs-trash-deleted-at = Verwijderd op { $date }
docs-trash-restore = Herstellen
docs-trash-delete-forever = Definitief verwijderen
docs-trash-confirm-delete = Verwijderen bevestigen?

# Documentation gaps (DocumentationGapsView): queue of open
# knowledge gaps with a list pane and detail pane.
docs-gaps-title = Kennislacunes
docs-gaps-heading = Kennislacunes
docs-gaps-back-docs = Documentatie
docs-gaps-back-list = Kennislacunes
docs-gaps-refresh = Signalen vernieuwen
docs-gaps-refreshing = Vernieuwen
docs-gaps-detect-no-results = Geen nieuwe clusters gevonden
docs-gaps-detect-created = { $count } nieuw
docs-gaps-detect-updated = { $count } bijgewerkt
docs-gaps-loading = Laden
docs-gaps-empty = Geen openstaande kennislacunes. Markeer een ticket vanuit de zijbalk om er een toe te voegen.
docs-gaps-impact-searches = zoekopdrachten
docs-gaps-impact-recent-tickets = recente tickets
docs-gaps-impact-tickets = tickets
docs-gaps-impact-tooltip = { $count } { $label } die vraag naar dit document aantonen
docs-gaps-signal-count = { $count ->
    [one] { $count } signaal
   *[other] { $count } signalen
   }
docs-gaps-select-prompt = Selecteer een lacune uit de lijst om het bewijs te bekijken.
docs-gaps-status-label = Status:
docs-gaps-last-evidence = Laatste bewijs: { $time }
docs-gaps-dismiss = Negeren
docs-gaps-evidence-heading = Bewijs
docs-gaps-evidence-empty = Geen bewijsregels.
docs-gaps-signal-manual-flag = Handmatige melding
docs-gaps-signal-ticket-cluster = Ticketcluster
docs-gaps-signal-failed-search = Mislukte zoekopdracht
docs-gaps-signal-stale-doc = Verouderd document
docs-gaps-signal-ai-suggested = AI-suggestie
docs-gaps-cluster-fallback = Cluster
docs-gaps-cluster-via = via { $channel }
docs-gaps-cluster-more = { $count ->
    [one] en nog { $count } meer
   *[other] en nog { $count } meer
   }
docs-gaps-stale-untitled = Naamloos document
docs-gaps-stale-verified = Geverifieerd { $time }
docs-gaps-stale-verified-no-time = Geverifieerd
docs-gaps-stale-days-past-due = { $count ->
    [one] { $count } dag over tijd
   *[other] { $count } dagen over tijd
   }
docs-gaps-stale-recent-tickets = { $count ->
    [one] recent gesloten ticket verwijst nog naar dit document:
   *[other] recent gesloten tickets verwijzen nog naar dit document:
   }
docs-gaps-stale-plus-more = + { $count } meer
docs-gaps-stale-auto-dismiss = Het document opnieuw verifiëren sluit deze lacune automatisch.
docs-gaps-failed-search-count = { $count ->
    [one] { $count } zoekopdracht zonder resultaten
   *[other] { $count } zoekopdrachten zonder resultaten
   }
docs-gaps-failed-search-range = eerste { $first }, laatste { $last }
docs-gaps-flagged-by = Gemarkeerd door { $name }
docs-gaps-resolve-heading = Deze lacune oplossen
docs-gaps-resolve-body = Open een van bovenstaande tickets en gebruik { $action } in de zijbalk. Het nieuwe document wordt automatisch gekoppeld als 'lost op' aan elk gemarkeerd ticket.
docs-gaps-resolve-action = Opslaan als document

# Document view (DocumentView): full-page editor for a single doc
# page (or a ticket note). Covers the header toolbar, metadata
# strip, save indicators, verification chips, panels, and toasts.
doc-detail-back-to-ticket = Terug naar ticket
doc-detail-back-to-documentation = Terug naar documentatie
doc-detail-saving = Bezig met opslaan
doc-detail-publish = Publiceren
doc-detail-star = Pagina markeren
doc-detail-unstar = Markering opheffen
doc-detail-copy-link = Link kopiëren
doc-detail-copied = Gekopieerd
doc-detail-untitled = Naamloos
doc-detail-status-draft = Concept
doc-detail-status-archived = Gearchiveerd
doc-detail-needs-verification = Verificatie nodig
doc-detail-needs-verification-title = Deze pagina verifiëren
doc-detail-verification-stale = Verificatie verouderd
doc-detail-verification-stale-title = Deze pagina opnieuw verifiëren
doc-detail-sse-live = Live updates actief
doc-detail-sse-connecting = Verbinden
doc-detail-sse-disconnected = Verbinding verbroken
doc-detail-history = Geschiedenis
doc-detail-history-title = Revisiegeschiedenis
doc-detail-editor-placeholder = Voer hier de documentatie-inhoud in
doc-detail-not-found-title = Document niet gevonden
doc-detail-not-found-body = Het document dat u zoekt bestaat niet of is verplaatst.
doc-detail-not-found-link = Ga naar documentatie-startpagina
doc-detail-toast-deleting = Document verwijderen
doc-detail-toast-deleted = Document succesvol verwijderd
doc-detail-toast-delete-error = Fout bij verwijderen document
doc-detail-duplicate-suffix = { $title } (kopie)
doc-detail-ticket-note-title = Notities voor ticket #{ $id }
doc-detail-ticket-note-description = Documentatie voor ticket { $title }
doc-detail-ticket-note-author-system = Systeem

# Asset planner (AssetPlannerView): kanban voor het plannen van
# apparaat-uitrol, gegroepeerd op OS-familie, garantieperiode of
# nalevingsstatus. Bevat de header, zijbalkfilters, groepkolommen
# en chips op de apparaatkaarten.
asset-planner-title = Apparatuur
asset-planner-subtitle = Plan uitrol op OS, garantie of naleving.
asset-planner-search-placeholder = Zoek op naam, hostnaam, model…
asset-planner-group-by = Groeperen op
asset-planner-axis-os = OS-familie
asset-planner-axis-warranty = Garantie
asset-planner-axis-compliance = Naleving
asset-planner-loading = Apparatuur laden…
asset-planner-load-error = Apparatuur laden mislukt
asset-planner-filters-heading = Filters
asset-planner-filters-clear = Wissen ({ $count })
asset-planner-section-os = OS
asset-planner-section-warranty = Garantie
asset-planner-section-compliance = Naleving
asset-planner-count = { $visible } van { $total ->
    [one] { $total } apparaat
   *[other] { $total } apparaten
   }
asset-planner-empty = Geen activa komen overeen met de huidige filters.
asset-planner-warranty-ends = Garantie loopt af op { $date }
asset-planner-no-warranty-data = Geen garantiegegevens
asset-planner-warranty-unknown-short = n.v.t.
asset-planner-card-host = Host
asset-planner-card-os = OS
asset-planner-card-model = Model
asset-planner-card-tag = Label
asset-planner-card-compliance = Naleving
asset-planner-os-windows = Windows
asset-planner-os-macos = macOS
asset-planner-os-linux = Linux
asset-planner-os-ios = iOS
asset-planner-os-android = Android
asset-planner-os-other = Overig
asset-planner-warranty-expired = Verlopen
asset-planner-warranty-expiring-30d = Verloopt binnen 30 dagen
asset-planner-warranty-expiring-90d = Verloopt binnen 90 dagen
asset-planner-warranty-active = Actief
asset-planner-warranty-unknown = Onbekend
asset-planner-compliance-unknown = Onbekend

# Collection view (CollectionView): documentation collection
# detail page with editable name/icon, an overview editor,
# visibility chips, an expandable list of pages with custom
# permissions, and the collection's page tree.
collection-back-to-documentation = Terug naar documentatie
collection-not-found-title = Collectie niet gevonden
collection-action-delete = Verwijderen
collection-action-manage-access = Toegang beheren
collection-action-new-page = Nieuwe pagina
collection-new-page-default-title = Nieuwe pagina
collection-not-found-heading = Collectie niet gevonden
collection-not-found-description = Deze collectie is mogelijk verplaatst of verwijderd.
collection-badge-system = Systeem
collection-badge-restricted = Beperkt
collection-badge-public = Openbaar
collection-overview-heading = Overzicht
collection-overview-placeholder = Schrijf een overzicht voor deze collectie...
collection-overrides-summary = { $count ->
    [one] { $count } pagina met aangepaste rechten
   *[other] { $count } pagina's met aangepaste rechten
   }
collection-pages-heading = Pagina's
collection-page-count = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
   }
collection-delete-title = { $name } verwijderen?
collection-delete-title-fallback = Collectie verwijderen?
collection-delete-message = De pagina's in deze collectie worden niet verwijderd.
collection-delete-confirm = Verwijderen

# CSV import (CsvImportView): admin page for importing users,
# devices, or tickets from CSV. Covers the page header, status
# messages, action buttons, the import status card, guideline
# panels, the template list, and both modals (file upload and
# template download).
csv-import-back = Terug naar Gegevensimport
csv-import-title = CSV-import
csv-import-subtitle = Importeer gegevens uit CSV-bestanden in je systeem

# Phase 1 wizard: upload -> review -> done.
csv-import-step-upload = Upload
csv-import-step-review = Review
csv-import-step-done = Done
csv-import-step-upload-heading = Upload your CSV
csv-import-type-label = What are you importing?
csv-import-type-assets = Assets
csv-import-type-users = Users
csv-import-type-tickets = Tickets
csv-import-type-coming-soon = coming soon
csv-import-template-label = Start with our template
csv-import-template-help = The CSV needs to use these exact column headers. Download the empty template, fill it in, then upload it here.
csv-import-template-button = Download template
csv-import-file-label = Choose your file
csv-import-drop-zone-idle = Drop a CSV here, or click to browse
csv-import-drop-here = Release to upload
csv-import-drop-zone-hint = .csv up to 10 MB
csv-import-drop-zone-replace = Click or drop another file to replace
csv-import-error-not-csv = "{ $name }" is not a CSV file
csv-import-action-validate = Validate
csv-import-summary-rows = Rows in file
csv-import-empty-file = This file has only a header row. Fill in some data rows below the header, save the CSV, and upload again.
csv-import-summary-create = Will create
csv-import-summary-update = Will update
csv-import-errors-heading = { $count } row(s) have errors
csv-import-errors-truncated = showing first 100
csv-import-errors-row = Row
csv-import-errors-column = Column
csv-import-errors-message = Message
csv-import-action-discard = Discard
csv-import-action-apply = Apply ({ $count } rows)
csv-import-action-new = New import
csv-import-action-view-assets = View assets
csv-import-action-view-users = View users
csv-import-action-view-tickets = View tickets
csv-import-done-heading = Import complete
csv-import-done-body = { $count } row(s) committed.
csv-import-error-generic = Import failed. Check the file and try again.
csv-import-error-commit-failed = Apply failed; rows were not committed.
csv-import-action-import = Gegevens importeren
csv-import-action-templates = Sjablonen downloaden
csv-import-status-heading = Importstatus
csv-import-status-success = Import voltooid
csv-import-status-in-progress = Import bezig
csv-import-status-error = Import mislukt
csv-import-last-import = Laatste import: { $date }
csv-import-results-total = Totaal aantal records
csv-import-results-successful = Geslaagd
csv-import-results-failed = Mislukt
csv-import-guidelines-heading = Richtlijnen voor CSV-import
csv-import-requirements-heading = Vereisten CSV-bestand
csv-import-requirements-utf8 = Bestanden moeten in CSV-formaat met UTF-8-codering zijn
csv-import-requirements-headers = De eerste rij moet kolomkoppen bevatten die overeenkomen met de verwachte velden
csv-import-requirements-required = Verplichte velden mogen niet leeg zijn
csv-import-requirements-date-format = Datumvelden moeten het formaat JJJJ-MM-DD gebruiken
csv-import-requirements-max-size = Maximale bestandsgrootte: 10 MB
csv-import-notes-heading = Belangrijke opmerkingen
csv-import-notes-updates = Bestaande records worden bijgewerkt als ze een unieke identifier delen (zoals e-mail of ID)
csv-import-notes-validation = Gegevensvalidatie wordt vóór de import uitgevoerd, records met ongeldige gegevens worden overgeslagen
csv-import-notes-duration = Grote imports kunnen enkele minuten duren
csv-import-notes-templates = Download en gebruik onze sjablonen voor de juiste opmaak
csv-import-templates-heading = Beschikbare sjablonen
csv-import-templates-intro = Gebruik deze sjablonen als startpunt voor je CSV-imports
csv-import-template-users-name = Sjabloon Gebruikers
csv-import-template-users-description = Importeer gebruikersaccounts met rollen en contactgegevens
csv-import-template-devices-name = Activasjabloon
csv-import-template-devices-description = Importeer activa met hardwaregegevens en eigendomsinformatie
csv-import-template-tickets-name = Sjabloon Tickets
csv-import-template-tickets-description = Importeer supporttickets met details en toegewezen personen
csv-import-template-download = Downloaden
csv-import-modal-import-title = Gegevens uit CSV importeren
csv-import-modal-data-type = Type gegevens
csv-import-modal-type-users = Gebruikers
csv-import-modal-type-devices = Activa
csv-import-modal-type-tickets = Tickets
csv-import-modal-file-label = CSV-bestand
csv-import-modal-upload-link = Een bestand uploaden
csv-import-modal-drag-drop = of versleep het hierheen
csv-import-modal-size-hint = CSV-bestanden tot 10 MB
csv-import-modal-cancel = Annuleren
csv-import-modal-start = Import starten
csv-import-modal-starting = Bezig met importeren...
csv-import-modal-templates-title = CSV-sjablonen
csv-import-modal-templates-intro = Download onze CSV-sjablonen zodat je gegevens correct worden opgemaakt voor de import.
csv-import-modal-fields-count = { $count ->
    [one] { $count } veld
   *[other] { $count } velden
   }
csv-import-modal-close = Sluiten
csv-import-error-no-file = Selecteer een bestand om te importeren
csv-import-error-failed = Import mislukt
csv-import-success-completed = Import succesvol voltooid
csv-import-toast-template-downloaded = Sjabloon { $type } gedownload

# Error page (ErrorView)
error-page-default-code = 404
error-page-default-message = Pagina niet gevonden
error-page-description = De pagina die je zoekt bestaat niet, of je hebt er mogelijk geen toegang toe.
error-page-go-back = Terug
error-page-go-home = Naar dashboard
error-page-debug-title = Debug-instellingen (druk op 'd' om te wisselen)
error-page-debug-master-toggle = Hoofdschakelaar effecten
error-page-debug-global-intensity = Algemene intensiteit
error-page-debug-channel-separation = Kanaalscheiding
error-page-debug-distortion-scale = Vervormingsschaal
error-page-debug-glitch-frequency = Glitch-frequentie
error-page-debug-glitch-intensity = Glitch-intensiteit
error-page-debug-cursor-influence = Cursorinvloed

# PDF viewer (PDFViewerView)
pdf-viewer-default-filename = Document
pdf-viewer-back = Terug
pdf-viewer-share = Delen
pdf-viewer-share-tooltip = Link naar klembord kopiëren
pdf-viewer-loading = PDF-document laden...
pdf-viewer-error-title = Kan PDF niet laden
pdf-viewer-error-go-back = Terug
pdf-viewer-error-no-source = Geen PDF-bron opgegeven
pdf-viewer-error-failed = Kan PDF niet laden
pdf-viewer-error-failed-with-reason = Kan PDF niet laden: { $reason }
pdf-viewer-error-unknown = Onbekende fout

# Settings: MFA (MFASettings) - tweefactorauthenticatie instellen, verifiëren, back-upcodes en uitschakelen
settings-mfa-title = Tweefactorauthenticatie
settings-mfa-title-success = Installatie voltooid!
settings-mfa-toggle-label = Tweefactorauthenticatie inschakelen
settings-mfa-toggle-description-enabled = Je account is beveiligd met 2FA
settings-mfa-toggle-description-disabled = Beveilig je account met een authenticator-app
settings-mfa-admin-status-enabled = Ingeschakeld
settings-mfa-admin-status-disabled = Niet ingeschakeld
settings-mfa-admin-backup-codes-generated = · Back-upcodes gegenereerd
settings-mfa-admin-disable = Uitschakelen
settings-mfa-admin-disabling = Uitschakelen...
settings-mfa-admin-note = Voor MFA-installatie is de authenticator-app van de accounthouder vereist.
settings-mfa-admin-disable-success = Tweefactorauthenticatie is uitgeschakeld voor deze gebruiker
settings-mfa-admin-disable-error = Kan MFA niet uitschakelen
settings-mfa-admin-load-error = Kan MFA-status van deze gebruiker niet laden
settings-mfa-setup-init-error = Kan MFA-installatie niet starten
settings-mfa-setup-not-ready = MFA-installatie is niet correct geïnitialiseerd
settings-mfa-manual-toggle = Kun je niet scannen? Voer de code handmatig in
settings-mfa-manual-instructions = Voer deze geheime sleutel in je authenticator-app in:
settings-mfa-copy-button = Kopiëren
settings-mfa-copied-button = Gekopieerd!
settings-mfa-copy-tooltip = Kopiëren naar klembord
settings-mfa-copied-tooltip = Gekopieerd naar klembord!
settings-mfa-copy-error = Kan niet naar klembord kopiëren
settings-mfa-verify-heading = Verificatiecode invoeren
settings-mfa-verify-instructions = Voer de 6-cijferige code uit je authenticator-app in:
settings-mfa-verify-aria-label = MFA-verificatiecode
settings-mfa-verify-button = Verifiëren
settings-mfa-verifying-button = Verifiëren...
settings-mfa-verify-invalid-length = Voer een geldige 6-cijferige code in
settings-mfa-verify-missing-secret = MFA-sleutel ontbreekt. Start het installatieproces opnieuw.
settings-mfa-verify-invalid-code = Ongeldige verificatiecode. Probeer het opnieuw.
settings-mfa-verify-incomplete-login = MFA ingeschakeld, maar de loginreactie was onvolledig
settings-mfa-qr-alt = MFA QR-code
settings-mfa-verifying-heading = Code verifiëren
settings-mfa-verifying-message = Even geduld terwijl we je authenticator-code verifiëren...
settings-mfa-disable-password-prompt = Voer je wachtwoord in om MFA uit te schakelen:
settings-mfa-backup-codes-heading = Back-upcodes
settings-mfa-backup-codes-description = Bewaar deze back-upcodes op een veilige plek. Je kunt ze gebruiken om bij je account te komen als je je authenticator-apparaat verliest.
settings-mfa-backup-codes-download = Downloaden
settings-mfa-backup-codes-download-tooltip = Back-upcodes als tekstbestand downloaden
settings-mfa-backup-codes-download-success = Back-upcodes gedownload
settings-mfa-backup-codes-download-error = Kan back-upcodes niet downloaden
settings-mfa-backup-file-title = Nosdesk Back-upcodes
settings-mfa-backup-file-warning = BELANGRIJK: bewaar deze back-upcodes op een veilige plek.
settings-mfa-backup-file-usage = Elke code kan slechts één keer worden gebruikt om bij je account te komen als je je authenticator-apparaat verliest.
settings-mfa-backup-file-codes-heading = Back-upcodes:
settings-mfa-backup-file-generated = Gegenereerd op: { $date }
settings-mfa-success-heading = Tweefactorauthenticatie ingeschakeld!
settings-mfa-success-message = Je account is nu beveiligd met 2FA. Je moet een code uit je authenticator-app invoeren bij het inloggen.
settings-mfa-success-cta = Aan de slag met Nosdesk!

# Settings: auth methods (AuthMethodsSettings) - gekoppelde aanmeldingsproviders en beheer van actieve sessies
settings-auth-methods-section-title = Aanmeldingsmethoden
settings-auth-methods-type-local = E-mail / Wachtwoord
settings-auth-methods-type-microsoft = Microsoft
settings-auth-methods-primary-badge = Primair
settings-auth-methods-added-suffix = · Toegevoegd op { $date }
settings-auth-methods-remove = Verwijderen
settings-auth-methods-connect-microsoft = Microsoft-account koppelen
settings-auth-methods-connect-microsoft-already = Al gekoppeld
settings-auth-methods-connect-microsoft-provider = Azure AD / Entra ID
settings-auth-methods-link-success = { $provider }-account gekoppeld
settings-auth-methods-link-error = Kan { $provider }-account niet koppelen
settings-auth-methods-remove-success = Aanmeldingsmethode verwijderd
settings-auth-methods-remove-error = Kan aanmeldingsmethode niet verwijderen
settings-auth-methods-sessions-section-title = Actieve sessies
settings-auth-methods-sessions-revoke-all = Alle andere intrekken
settings-auth-methods-sessions-unknown-device = Onbekend apparaat
settings-auth-methods-sessions-unknown-location = Onbekende locatie
settings-auth-methods-sessions-current-badge = Huidige
settings-auth-methods-sessions-last-active = { $location } • Laatst actief { $date }
settings-auth-methods-sessions-revoke = Intrekken
settings-auth-methods-sessions-revoke-success = Sessie ingetrokken
settings-auth-methods-sessions-revoke-error = Kan sessie niet intrekken
settings-auth-methods-sessions-revoke-all-success = Alle andere sessies zijn ingetrokken
settings-auth-methods-sessions-revoke-all-error = Kan sessies niet intrekken
settings-auth-methods-sessions-load-error = Kan actieve sessies niet laden

# Gemeenschappelijke onderdelen.
common-modal-close = Venster sluiten
form-textarea-resize-grip-label = Sleep om te schalen

# Editor: werkbalk (CollaborativeEditor)
editor-toolbar-text-style = Tekststijl
editor-toolbar-bold = Vet
editor-toolbar-italic = Cursief
editor-toolbar-bullet-list = Opsomming
editor-toolbar-numbered-list = Genummerde lijst
editor-toolbar-insert = Invoegen
editor-toolbar-undo = Ongedaan maken
editor-toolbar-redo = Opnieuw uitvoeren
editor-toolbar-revision-history = Revisiegeschiedenis
editor-toolbar-editing-with = Bewerken met:
editor-toolbar-connection-connecting = Verbinden...
editor-toolbar-connection-disconnected = Verbinding verbroken
editor-toolbar-user-title = { $name }
editor-toolbar-user-title-uuid = { $name } (UUID: { $uuid })

# Editor: menu voor tekststijl (CollaborativeEditor)
editor-type-menu-plain = Standaard
editor-type-menu-heading-1 = Kop 1
editor-type-menu-heading-2 = Kop 2
editor-type-menu-heading-3 = Kop 3
editor-type-menu-blockquote = Citaat
editor-type-menu-code-block = Codeblok

# Editor: invoegmenu (CollaborativeEditor)
editor-insert-menu-bullet-list = Opsomming
editor-insert-menu-numbered-list = Genummerde lijst
editor-insert-menu-blockquote = Citaat
editor-insert-menu-code-block = Codeblok
editor-insert-menu-link = Link
editor-insert-menu-embed-document = Document insluiten

# Editor: prompt voor codeblok-taal (CollaborativeEditor)
editor-code-block-language-prompt = Taal voor syntaxisaccentuering (optioneel):

# Editor: vermeldingenmenu (CollaborativeEditor)
editor-mention-searching = Zoeken naar "{ $query }"
editor-mention-no-results = Geen gebruikers gevonden
editor-mention-hint-navigate = Navigeren
editor-mention-hint-select = Selecteren
editor-mention-hint-close = Sluiten

# Editor: link-tooltip (LinkTooltip)
editor-link-tooltip-placeholder = URL invoeren...
editor-link-tooltip-apply = Toepassen
editor-link-tooltip-cancel = Annuleren
editor-link-tooltip-edit = Link bewerken
editor-link-tooltip-remove = Link verwijderen

# Editor: documentkiezer (DocumentPicker)
editor-doc-picker-title = Document insluiten
editor-doc-picker-close = Sluiten
editor-doc-picker-search-placeholder = Documenten zoeken...
editor-doc-picker-empty = Geen documenten gevonden.

# Editor: revisiegeschiedenis-paneel (RevisionHistory)
editor-revision-history-title = Revisiegeschiedenis

# Editor: revisielijst (RevisionList)
editor-revisions-empty-title = Nog geen revisies
editor-revisions-empty-hint = Revisies worden aangemaakt wanneer je wijzigingen aanbrengt
editor-revisions-current-version = Huidige versie
editor-revisions-by = Door:
editor-revisions-more-contributors = +{ $count }
editor-revisions-word-count = { $count } { $count ->
    [one] woord
   *[other] woorden
  }
editor-revisions-restore-button = Deze versie herstellen
editor-revisions-restoring = Herstellen...
editor-revisions-unknown-user = Onbekend
editor-revisions-load-error = Kan revisies niet laden
editor-revisions-restore-error = Kan revisie niet herstellen
editor-revisions-just-now = Zojuist
editor-revisions-minutes-ago = { $minutes } min geleden
editor-revisions-hours-ago = { $hours } u geleden
editor-revisions-days-ago = { $days } d geleden
editor-revisions-confirm-title = Revisie herstellen?
editor-revisions-confirm-body = Hiermee wordt het ticket teruggezet naar revisie { $revision }. De huidige inhoud wordt vervangen door de geselecteerde revisie.
editor-revisions-confirm-note = Let op: er wordt een nieuwe revisie aangemaakt, zodat je deze wijziging altijd ongedaan kunt maken.
editor-revisions-confirm-cancel = Annuleren
editor-revisions-confirm-restore = Herstellen
# Ticket media: bijlagevoorvertoning (AttachmentPreview)
ticket-media-attachment-voice-message = Spraakbericht
ticket-media-attachment-file-fallback = Bestand
ticket-media-attachment-pdf-document = PDF-document.{ $ext }
ticket-media-attachment-video = Video.{ $ext }
ticket-media-attachment-audio = Audio.{ $ext }
ticket-media-attachment-image = Afbeelding.{ $ext }
ticket-media-attachment-file = Bestand.{ $ext }
ticket-media-attachment-download = Bijlage downloaden
ticket-media-attachment-download-image = Afbeelding downloaden
ticket-media-attachment-download-animated = Geanimeerde afbeelding downloaden
ticket-media-attachment-download-pdf = PDF downloaden
ticket-media-attachment-delete-audio = Audio verwijderen
ticket-media-attachment-delete-video = Video verwijderen
ticket-media-attachment-delete-image = Afbeelding verwijderen
ticket-media-attachment-delete-pdf = PDF verwijderen
ticket-media-attachment-delete-file = Bestand verwijderen
ticket-media-attachment-format-unsupported = Dit afbeeldingsformaat wordt niet ondersteund door je browser
ticket-media-attachment-loading-pdf = PDF laden
ticket-media-attachment-animated-badge = GEANIMEERD
ticket-media-attachment-cancel = Annuleren
ticket-media-attachment-submit-video = Video versturen
ticket-media-attachment-preview-title-animated = Voorvertoning geanimeerde afbeelding
ticket-media-attachment-preview-title-image = Voorvertoning afbeelding

# Ticket media: audiospeler (AudioPlayer)
ticket-media-audio-play = Afspelen
ticket-media-audio-pause = Pauze
ticket-media-audio-loading = Laden...
ticket-media-audio-transcription = Transcriptie

# Ticket media: dictafoon (VoiceRecorder)
ticket-media-voice-recording = Opname
ticket-media-voice-cancel = Annuleren
ticket-media-voice-stop = Opname stoppen
ticket-media-voice-mic-error = Geen toegang tot de microfoon. Controleer je machtigingen.

# Ticket media: videospeler (VideoPlayer)
ticket-media-video-play = Afspelen
ticket-media-video-pause = Pauze
ticket-media-video-mute = Dempen
ticket-media-video-unmute = Dempen opheffen
ticket-media-video-fullscreen-enter = Volledig scherm openen
ticket-media-video-fullscreen-exit = Volledig scherm sluiten

# Ticket media: PDF-viewer (PDFViewer)
ticket-media-pdf-aria = PDF-viewer
ticket-media-pdf-loading = PDF laden...
ticket-media-pdf-zoom-out = Uitzoomen
ticket-media-pdf-zoom-out-aria = Uitzoomen
ticket-media-pdf-zoom-in = Inzoomen
ticket-media-pdf-zoom-in-aria = Inzoomen
ticket-media-pdf-fit-width = Aanpassen aan breedte
ticket-media-pdf-fit-width-aria = Aanpassen aan breedte
ticket-media-pdf-fullscreen = Volledig scherm
ticket-media-pdf-fullscreen-aria = Volledig scherm openen
ticket-media-pdf-download = PDF downloaden
ticket-media-pdf-download-aria = PDF downloaden

# Ticket media: bestandsvoorvertoning (FilePreview)
ticket-media-file-fallback = Bestand
ticket-media-file-pdf = PDF-document.{ $ext }
ticket-media-file-word = Word-document.{ $ext }
ticket-media-file-excel = Excel-werkblad.{ $ext }
ticket-media-file-powerpoint = Presentatie.{ $ext }
ticket-media-file-image = Afbeelding.{ $ext }
ticket-media-file-archive = Archief.{ $ext }
ticket-media-file-text = Tekstdocument.{ $ext }
ticket-media-file-generic = Bestand.{ $ext }
ticket-media-file-delete = Bestand verwijderen
ticket-media-file-download = Downloaden
ticket-media-file-thumbnail-error = Kon geen miniatuur maken
ticket-media-file-image-error = Kon afbeelding niet laden
ticket-media-file-animated-badge = GEANIMEERD

# Ticket picker: gebruikerskiezer (UserPicker)
ticket-picker-user-placeholder-assignee = Toewijzen aan...
ticket-picker-user-placeholder-requester = Gebruiker zoeken...
ticket-picker-user-search-staff = Personeel zoeken...
ticket-picker-user-search-users = Gebruikers zoeken...
ticket-picker-user-sheet-title-assignee = Toewijzen aan
ticket-picker-user-sheet-title-requester = Gebruiker zoeken
ticket-picker-user-listbox-assignees = Toewijsbare gebruikers
ticket-picker-user-listbox-users = Gebruikers
ticket-picker-user-loading-assignee = Toewijzingen laden
ticket-picker-user-loading-requester = Aanvragers laden
ticket-picker-user-view-profile = Profiel van { $name } bekijken
ticket-picker-user-clear = Selectie wissen
ticket-picker-user-empty-assignees = Nog geen toewijsbare gebruikers.
ticket-picker-user-empty-users = Geen gebruikers gevonden.
ticket-picker-user-empty-search = Geen gebruikers gevonden voor "{ $query }"
ticket-picker-user-section-selected-assignee = Momenteel toegewezen
ticket-picker-user-section-selected-requester = Huidige aanvrager
ticket-picker-user-section-you = Jij
ticket-picker-user-section-recent = Recent
ticket-picker-user-section-results = Resultaten
ticket-picker-user-section-staff = Personeel
ticket-picker-user-section-all = Alle gebruikers
ticket-picker-user-you-suffix = (jij)

# Ticket picker: gekoppeld ticket modaal (LinkedTicketModal)
ticket-picker-linked-title = Ticket koppelen
ticket-picker-linked-search-placeholder = Tickets zoeken...
ticket-picker-linked-loading = Tickets laden...
ticket-picker-linked-error = Kan tickets niet laden
ticket-picker-linked-try-again = Opnieuw proberen
ticket-picker-linked-empty-search = Geen tickets gevonden voor je zoekopdracht
ticket-picker-linked-empty = Geen tickets om te koppelen
ticket-picker-linked-col-id = ID
ticket-picker-linked-col-title = Titel
ticket-picker-linked-col-status = Status
ticket-picker-linked-col-requester = Aanvrager
ticket-picker-linked-col-updated = Bijgewerkt
ticket-picker-linked-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
}
ticket-picker-linked-cancel = Annuleren

# Ticket picker: standaardantwoordenkiezer (CannedResponsePicker)
ticket-picker-canned-trigger-aria = Standaardantwoord invoegen
ticket-picker-canned-trigger-title = Standaardantwoord invoegen ({ $shortcut })
ticket-picker-canned-listbox-aria = Standaardantwoorden
ticket-picker-canned-loading = Laden…
ticket-picker-canned-empty-title = Nog geen standaardantwoorden.
ticket-picker-canned-empty-hint = Beheerders kunnen sjablonen toevoegen in het beheergedeelte.
ticket-picker-canned-load-error = Kan sjablonen niet laden
ticket-picker-canned-search-placeholder = Zoek een standaardantwoord…
ticket-picker-canned-search-aria = Zoek standaardantwoord
ticket-picker-canned-no-matches = Geen resultaten voor "{ $query }"
ticket-picker-canned-missing-vars = Dit sjabloon gebruikt {"{{"}{ $names }{"}}"} waarvoor het huidige ticket geen waarde heeft. Die plekken blijven leeg.

# Ticket picker: apparaat modaal (DeviceModal)
ticket-picker-device-title = Activum toevoegen
ticket-picker-device-name-label = Naam
ticket-picker-device-name-placeholder = Voer activumnaam in
ticket-picker-device-hostname-label = Hostnaam
ticket-picker-device-hostname-placeholder = Voer hostnaam in
ticket-picker-device-serial-label = Serienummer
ticket-picker-device-serial-placeholder = Voer serienummer in
ticket-picker-device-model-label = Model
ticket-picker-device-model-placeholder = Voer model in
ticket-picker-device-warranty-label = Garantiestatus
ticket-picker-device-warranty-active = Actief
ticket-picker-device-warranty-warning = Waarschuwing
ticket-picker-device-warranty-expired = Verlopen
ticket-picker-device-warranty-unknown = Onbekend
ticket-picker-device-cancel = Annuleren
ticket-picker-device-add = Activum toevoegen

# Documentpictogramkiezer (DocumentIconSelector)
doc-icon-selector-trigger-aria = Documentpictogram kiezen
doc-icon-selector-search-placeholder = Pictogrammen zoeken...
doc-icon-selector-empty = Geen pictogrammen gevonden
doc-icon-selector-footer-hint = Klik op een pictogram om te kiezen
doc-icon-selector-random = Willekeurig
doc-icon-selector-scroll-dot-aria = Naar sectie { $index } scrollen
doc-icon-selector-category-suggested = Voorgesteld
doc-icon-selector-category-documents = Documenten
doc-icon-selector-category-objects = Objecten
doc-icon-selector-category-symbols = Symbolen
doc-icon-selector-category-nature = Natuur
doc-icon-selector-category-animals = Dieren
doc-icon-selector-category-people = Mensen
doc-icon-selector-category-travel = Reizen
doc-icon-selector-category-food = Eten
doc-icon-selector-category-activities = Activiteiten
# Instellingen: profiel (UserProfileCard)
settings-profile-banner-alt = Profielbanner
settings-profile-change-photo = Foto wijzigen
settings-profile-name-placeholder = Voer een naam in...
settings-profile-pronouns-label = Voornaamwoorden
settings-profile-pronouns-placeholder = Voornaamwoorden toevoegen (bijv. hij/hem, zij/haar, die/hen)
settings-profile-save = Opslaan
settings-profile-signature-label = E-mailhandtekening
settings-profile-signature-hint-prefix = Wordt toegevoegd aan uw uitgaande antwoorden op tickets uit een kanaal (e-mail). Het standaardscheidingsteken is
settings-profile-signature-hint-suffix = .
settings-profile-signature-variables-hint = Variabelen (per antwoord ingevuld):
settings-profile-signature-placeholder = Naam van agent
    IT-ondersteuning
settings-profile-unknown-user = Onbekende gebruiker
settings-profile-role-developer = Ontwikkelaar
settings-profile-role-admin = Beheerder
settings-profile-role-technician = Agent
settings-profile-role-user = Gebruiker
settings-profile-error-invalid-file = Ongeldig bestand
settings-profile-error-process-image = Kan afbeelding niet verwerken
settings-profile-error-user-uuid-missing = UUID van gebruiker niet gevonden
settings-profile-error-not-authenticated = Gebruiker niet geauthenticeerd
settings-profile-avatar-upload-success = Profielfoto bijgewerkt
settings-profile-banner-upload-success = Omslagafbeelding bijgewerkt
settings-profile-avatar-upload-error = Kan avatar niet uploaden
settings-profile-banner-upload-error = Kan banner niet uploaden
settings-profile-avatar-update-error = Kan avatar niet bijwerken
settings-profile-banner-update-error = Kan banner niet bijwerken
settings-profile-name-update-success = Naam bijgewerkt
settings-profile-name-update-error = Kan naam niet bijwerken
settings-profile-pronouns-update-success = Voornaamwoorden bijgewerkt
settings-profile-pronouns-update-error = Kan voornaamwoorden niet bijwerken
settings-profile-signature-update-success = Handtekening bijgewerkt
settings-profile-signature-update-error = Kan handtekening niet bijwerken

# Instellingen: meldingen (NotificationSettings)
settings-notifications-category-ticket-label = Tickets
settings-notifications-category-ticket-description = Meldingen over tickettoewijzingen en statuswijzigingen
settings-notifications-category-comment-label = Opmerkingen
settings-notifications-category-comment-description = Meldingen wanneer iemand reageert op uw tickets
settings-notifications-category-mention-label = Vermeldingen
settings-notifications-category-mention-description = Meldingen wanneer iemand u vermeldt
settings-notifications-category-documentation-label = Documentatie
settings-notifications-category-documentation-description = Meldingen over wijzigingen aan documentatiepagina's
settings-notifications-channel-in-app-name = In-app
settings-notifications-channel-in-app-description = Toastmeldingen tijdens het gebruik van de app
settings-notifications-channel-email-name = E-mail
settings-notifications-channel-email-description = E-mailmeldingen (snelheidsbeperkt)
settings-notifications-type-ticket-assigned-name = Ticket toegewezen
settings-notifications-type-ticket-assigned-description = Wanneer u aan een ticket wordt toegewezen
settings-notifications-type-ticket-status-changed-name = Status gewijzigd
settings-notifications-type-ticket-status-changed-description = Wanneer een ticket waarbij u betrokken bent van status verandert
settings-notifications-type-comment-added-name = Nieuwe opmerking
settings-notifications-type-comment-added-description = Wanneer iemand reageert op uw ticket
settings-notifications-type-mentioned-name = Vermeld
settings-notifications-type-mentioned-description = Wanneer iemand u in een opmerking vermeldt
settings-notifications-type-ticket-created-requester-name = Ticket aangemaakt
settings-notifications-type-ticket-created-requester-description = Wanneer een ticket namens u wordt aangemaakt
settings-notifications-type-doc-page-updated-name = Pagina bijgewerkt
settings-notifications-type-doc-page-updated-description = Wanneer een documentatiepagina waarop u bent geabonneerd wordt gewijzigd
settings-notifications-type-asset-low-stock-name = Low Stock Alert
settings-notifications-type-asset-low-stock-description = When a stock-tracked asset drops to or below its configured low-stock threshold
settings-notifications-browser-banner-title = Browsermeldingen inschakelen
settings-notifications-browser-banner-description = Sta browsermeldingen toe om waarschuwingen te ontvangen, ook als de app niet actief is.
settings-notifications-browser-banner-enable = Meldingen inschakelen
settings-notifications-browser-enabled-success = Browsermeldingen ingeschakeld
settings-notifications-browser-denied-error = Toestemming voor browsermeldingen geweigerd
settings-notifications-quick-settings-title = Snelle instellingen
settings-notifications-channel-toggle-all-label = Alle { $channel }-meldingen
settings-notifications-column-header = Melding
settings-notifications-load-error = Kan meldingsvoorkeuren niet laden
settings-notifications-preference-update-success = Voorkeur bijgewerkt
settings-notifications-preference-update-error = Kan voorkeur niet bijwerken
settings-notifications-channel-bulk-success = Alle { $channel }-meldingen { $state ->
    [enabled] ingeschakeld
   *[disabled] uitgeschakeld
}
settings-notifications-info-footer = E-mailmeldingen zijn snelheidsbeperkt om uw inbox niet te overspoelen. U ontvangt hoogstens één e-mail per ticket per 5 minuten.

# Instellingen: passkeys (PasskeySettings)
settings-passkey-section-title = Passkeys
settings-passkey-empty-title = Geen passkeys geregistreerd
settings-passkey-empty-admin-description = Deze gebruiker heeft geen passkeys geregistreerd.
settings-passkey-empty-self-description = Log in met biometrie of een beveiligingssleutel in plaats van een wachtwoord
settings-passkey-add-button = Passkey toevoegen
settings-passkey-add-another-button = Nog een passkey toevoegen
settings-passkey-synced-badge = Gesynchroniseerd
settings-passkey-last-used = Laatst gebruikt { $date }
settings-passkey-never-used = Nooit gebruikt
settings-passkey-rename-tooltip = Passkey hernoemen
settings-passkey-delete-tooltip = Passkey verwijderen
settings-passkey-admin-info = Het registreren van een passkey vereist de biometrie of beveiligingssleutel van de accounteigenaar.
settings-passkey-unsupported-title = Browser niet ondersteund
settings-passkey-unsupported-description = Uw browser ondersteunt geen passkeys (WebAuthn). Gebruik een moderne browser zoals Chrome, Safari, Firefox of Edge.
settings-passkey-admin-load-error = Kan passkeys voor deze gebruiker niet laden
settings-passkey-admin-delete-success = Passkey is verwijderd
settings-passkey-admin-delete-error = Kan passkey niet verwijderen
settings-passkey-add-modal-title = Passkey toevoegen
settings-passkey-add-modal-description = Geef uw passkey een naam zodat u hem later kunt herkennen. Uw apparaat vraagt u de passkey aan te maken.
settings-passkey-add-modal-name-label = Naam van passkey (optioneel)
settings-passkey-add-modal-name-placeholder = bijv. MacBook Pro, iPhone
settings-passkey-modal-cancel = Annuleren
settings-passkey-add-modal-create = Passkey aanmaken
settings-passkey-add-modal-creating = Bezig met aanmaken...
settings-passkey-rename-modal-title = Passkey hernoemen
settings-passkey-rename-modal-name-label = Naam van passkey
settings-passkey-rename-modal-placeholder = Voer een nieuwe naam in
settings-passkey-rename-modal-save = Opslaan
settings-passkey-delete-modal-title = Passkey verwijderen
settings-passkey-delete-modal-confirm-prefix = Weet u zeker dat u
settings-passkey-delete-modal-confirm-suffix = wilt verwijderen? U kunt deze passkey dan niet meer gebruiken om in te loggen.
settings-passkey-delete-modal-password-label = Voer uw wachtwoord in om te bevestigen
settings-passkey-delete-modal-password-placeholder = Uw wachtwoord
settings-passkey-delete-modal-confirm = Passkey verwijderen
settings-passkey-admin-delete-modal-confirm-prefix = Weet u zeker dat u
settings-passkey-admin-delete-modal-confirm-suffix = wilt verwijderen? Deze gebruiker kan dan niet meer inloggen met deze passkey.
settings-passkey-admin-delete-modal-deleting = Bezig met verwijderen...

# Authenticatie: passkey-instelling (PasskeySetup)
auth-passkey-setup-unsupported-title = Passkeys niet beschikbaar
auth-passkey-setup-unsupported-insecure = Passkeys vereisen een beveiligde verbinding (HTTPS). U bevindt zich op een onbeveiligde verbinding.
auth-passkey-setup-unsupported-browser = Uw browser ondersteunt geen passkeys. Gebruik een moderne browser zoals Chrome, Safari, Firefox of Edge, of kies in plaats daarvan de optie authenticator-app.
auth-passkey-setup-heading = Passkey instellen
auth-passkey-setup-description = Log veilig in met Face ID, Touch ID, Windows Hello of een beveiligingssleutel.
auth-passkey-setup-name-label = Naam van passkey
auth-passkey-setup-name-placeholder = bijv. MacBook Pro, iPhone
auth-passkey-setup-name-hint = Een naam om deze passkey later te herkennen
auth-passkey-setup-create-button = Passkey aanmaken
auth-passkey-setup-creating-button = Passkey aanmaken...
auth-passkey-setup-device-iphone = iPhone
auth-passkey-setup-device-ipad = iPad
auth-passkey-setup-device-mac = Mac
auth-passkey-setup-device-windows = Windows-pc
auth-passkey-setup-device-android = Android-apparaat
auth-passkey-setup-device-linux = Linux-pc
auth-passkey-setup-device-generic = Dit apparaat
auth-passkey-setup-error-session-expired = Sessie verlopen. Log opnieuw in.
auth-passkey-setup-error-cancelled = Registratie is geannuleerd of niet toegestaan
auth-passkey-setup-error-already-registered = Deze passkey is al geregistreerd
auth-passkey-setup-error-cancelled-generic = Registratie is geannuleerd
auth-passkey-setup-error-generic = Kan passkey niet registreren
auth-passkey-setup-success-message = Passkey aangemaakt
auth-passkey-setup-backup-codes-title = Bewaar uw herstelcodes
auth-passkey-setup-backup-codes-description = Als u geen toegang meer hebt tot uw passkey, kunt u een van deze codes gebruiken om in te loggen. Elke code kan slechts één keer worden gebruikt.
auth-passkey-setup-backup-codes-copy = Kopiëren
auth-passkey-setup-backup-codes-copied = Gekopieerd!
auth-passkey-setup-backup-codes-download = Downloaden
auth-passkey-setup-backup-codes-acknowledge = Ik heb mijn herstelcodes opgeslagen
auth-passkey-setup-backup-file-title = Nosdesk herstelcodes
auth-passkey-setup-backup-file-intro = Bewaar deze codes op een veilige plek. Elke code kan slechts één keer worden gebruikt.
auth-passkey-setup-success-heading = Passkey aangemaakt!
auth-passkey-setup-success-description = Uw passkey "{ $name }" is klaar voor gebruik.
auth-passkey-setup-success-protected-title = Uw account is beschermd
auth-passkey-setup-success-protected-description = De volgende keer dat u inlogt, gebruikt u eenvoudig uw vingerafdruk, gezicht of beveiligingssleutel in plaats van een wachtwoord.
auth-passkey-setup-success-cta = Aan de slag met Nosdesk!

# Instellingen: e-mailadressen (UserEmailsCard)
settings-emails-section-title = E-mailadressen
settings-emails-add-button = E-mail toevoegen
settings-emails-add-form-title = Nieuw e-mailadres toevoegen
settings-emails-add-placeholder = email@voorbeeld.com
settings-emails-add-submit = Toevoegen
settings-emails-add-submitting = Bezig met toevoegen...
settings-emails-add-cancel = Annuleren
settings-emails-empty = Geen e-mailadressen gevonden
settings-emails-primary-badge = Primair
settings-emails-verified-badge = Geverifieerd
settings-emails-unverified-badge = Niet geverifieerd
settings-emails-type-personal = persoonlijk
settings-emails-set-primary = Instellen als primair
settings-emails-remove = Verwijderen
settings-emails-confirm-title = E-mailadres verwijderen?
settings-emails-confirm-message = { $email } wordt niet meer aan dit account gekoppeld.
settings-emails-confirm-label = Verwijderen
settings-emails-error-required = E-mailadres is vereist
settings-emails-error-invalid-format = Ongeldig e-mailformaat
settings-emails-add-success = E-mailadres toegevoegd
settings-emails-add-error = Kan e-mailadres niet toevoegen
settings-emails-set-primary-success = { $email } ingesteld als primair e-mailadres
settings-emails-set-primary-error = Kan e-mailadres niet instellen als primair
settings-emails-delete-success = E-mailadres verwijderd
settings-emails-delete-error = Kan e-mailadres niet verwijderen
# Docs: artikelkaart (ArticleCard)
docs-article-card-updated = Bijgewerkt op { $date }
docs-article-card-edit = Artikel bewerken

# Docs: collectiebeheer (CollectionManager)
docs-collection-manager-title = Collecties beheren
docs-collection-manager-empty = Geen collecties beschikbaar.
docs-collection-manager-pages = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
}
docs-collection-manager-system-badge = Systeem
docs-collection-manager-cancel = Annuleren
docs-collection-manager-save = Opslaan
docs-collection-manager-saving = Opslaan...

# Docs: collectie-overzicht (CollectionBrowser)
docs-collection-browser-heading = Collecties
docs-collection-browser-new = Nieuw
docs-collection-browser-name-placeholder = Naam van de collectie...
docs-collection-browser-cancel = Annuleren
docs-collection-browser-create = Aanmaken
docs-collection-browser-loading-label = Collecties laden
docs-collection-browser-pages = { $count ->
    [one] { $count } pagina
   *[other] { $count } pagina's
}
docs-collection-browser-system-badge = Systeem
docs-collection-browser-restricted-badge = Beperkt
docs-collection-browser-empty = Nog geen collecties.

# Docs: boomelement (CollectionTreeItem)
docs-collection-tree-item-untitled = Naamloos
docs-collection-tree-item-draft = Concept
docs-collection-tree-item-override-title = Aangepaste rechten

# Docs: boomstructuur (CollectionTreeList)
docs-collection-tree-list-empty = Nog geen pagina's in deze collectie.

# Docs: zichtbaarheid collectie (CollectionVisibilityModal)
docs-collection-visibility-title = Toegang tot collectie
docs-collection-visibility-description = Selecteer welke groepen en gebruikers toegang hebben tot deze collectie. Een lege selectie maakt de collectie openbaar (zichtbaar voor iedereen).
docs-collection-visibility-public = Openbaar, zichtbaar voor alle gebruikers
docs-collection-visibility-picker-placeholder = Zoek gebruikers en groepen...
docs-collection-visibility-cancel = Annuleren
docs-collection-visibility-save = Opslaan
docs-collection-visibility-saving = Opslaan...

# Docs: actiemenu document (DocumentActionsMenu)
docs-actions-menu-subscribe = Abonneren
docs-actions-menu-unsubscribe = Afmelden
docs-actions-menu-insights = Inzichten
docs-actions-menu-history = Revisiegeschiedenis
docs-actions-menu-print = Afdrukken
docs-actions-menu-duplicate = Dupliceren
docs-actions-menu-export = Markdown downloaden
docs-actions-menu-move = Verplaatsen naar...
docs-actions-menu-collections = Collecties
docs-actions-menu-archive = Archiveren
docs-actions-menu-unarchive = Uit archief halen
docs-actions-menu-permissions = Rechten
docs-actions-menu-publish = Publiceren
docs-actions-menu-unpublish = Publicatie intrekken
docs-actions-menu-trash = Verplaatsen naar prullenbak
docs-actions-menu-trash-confirm = Prullenbak bevestigen?
docs-actions-menu-trigger = Pagina-acties

# Docs: kruimelpad (DocumentationBreadcrumb)
docs-breadcrumb-root = Documentatie
docs-breadcrumb-aria = Kruimelpad

# Docs: kaart (DocumentationCard)
docs-card-empty-content = Nog geen inhoud
docs-card-children-more = +{ $count } meer
docs-card-relative-unknown = Onbekend
docs-card-relative-today = Vandaag
docs-card-relative-yesterday = Gisteren
docs-card-relative-days = { $count }d geleden
docs-card-relative-weeks = { $count }w geleden
docs-card-freshness-fresh = Recent bijgewerkt
docs-card-freshness-recent = Deze week bijgewerkt
docs-card-freshness-stale = Niet recent bijgewerkt

# Docs: kaartskelet (DocumentationCardSkeleton)
docs-card-skeleton-label = Pagina's laden

# Docs: rijskelet (DocumentationRowSkeleton)
docs-row-skeleton-label = Pagina's laden

# Docs: navigatie (DocumentationNav)
docs-nav-starred = Met ster
docs-nav-empty = Nog geen documenten
docs-nav-sort-manual = Handmatig
docs-nav-sort-alpha = Alfabetisch
docs-nav-sort-recent = Recent bijgewerkt
docs-nav-untitled = Naamloos
docs-nav-duplicate-suffix = { $title } (kopie)
docs-nav-confirm-delete-collection-title = { $name } verwijderen?
docs-nav-confirm-delete-collection-fallback = Collectie verwijderen?
docs-nav-confirm-delete-collection-message = Pagina's in deze collectie worden naar de prullenbak verplaatst. Je kunt ze daar herstellen.
docs-nav-confirm-delete = Verwijderen
docs-nav-menu-open-new-tab = Openen in nieuw tabblad
docs-nav-menu-copy-link = Link kopiëren
docs-nav-menu-copy-md = Kopiëren als Markdown
docs-nav-menu-copy-text = Kopiëren als platte tekst
docs-nav-menu-add-child = Onderliggende pagina toevoegen
docs-nav-menu-star = Ster geven
docs-nav-menu-unstar = Ster verwijderen
docs-nav-menu-subscribe = Abonneren
docs-nav-menu-duplicate = Dupliceren
docs-nav-menu-move = Verplaatsen naar...
docs-nav-menu-history = Revisiegeschiedenis
docs-nav-menu-insights = Inzichten
docs-nav-menu-export-md = Markdown downloaden
docs-nav-menu-print = Afdrukken
docs-nav-menu-permissions = Rechten
docs-nav-menu-archive = Archiveren
docs-nav-menu-restore = Herstellen
docs-nav-menu-trash = Verplaatsen naar prullenbak
docs-nav-col-edit = Collectie bewerken
docs-nav-col-sort-heading = Sorteren op
docs-nav-col-permissions = Rechten
docs-nav-col-delete = Verwijderen

# Docs: rij-acties (NavRowActions)
docs-nav-row-more = Meer acties voor { $label }
docs-nav-row-add = Nieuwe pagina toevoegen aan { $label }

# Docs: navigatie-item (DocumentationNavItem)
docs-nav-item-draft = Concept

# Docs: boomelement (DocumentationTreeItem)
docs-tree-item-expand = Uitvouwen
docs-tree-item-collapse = Invouwen

# Docs: inhoudsopgave-item (DocumentationTocItem)
docs-toc-item-untitled = Naamloze pagina

# Docs: inzichtenpaneel (DocumentInsightsPanel)
docs-insights-title = Inzichten
docs-insights-source-heading = Bron
docs-insights-stats-heading = Statistieken
docs-insights-contributors-heading = Bijdragers
docs-insights-created = Aangemaakt { $relative }
docs-insights-updated = Laatst bijgewerkt { $relative }
docs-insights-reading-time = { $minutes ->
    [one] { $minutes } minuut leestijd
   *[other] { $minutes } minuten leestijd
}
docs-insights-word-count = { $count } woorden
docs-insights-char-count = { $count } tekens
docs-insights-emoji-count = { $count } emoji
docs-insights-contributors-loading = Bijdragers laden...
docs-insights-contributors-empty = Nog geen bijdragers.
docs-insights-contributor-role = Bijdrager
docs-insights-unknown-user = Onbekende gebruiker
docs-insights-relative-unknown = onbekend
docs-insights-relative-just-now = zojuist
docs-insights-relative-minutes = { $count } min geleden
docs-insights-relative-hours = { $count } u geleden
docs-insights-relative-days = { $count ->
    [one] { $count } dag geleden
   *[other] { $count } dagen geleden
}
docs-insights-relative-months = { $count ->
    [one] { $count } maand geleden
   *[other] { $count } maanden geleden
}
docs-insights-relative-years = { $count ->
    [one] { $count } jaar geleden
   *[other] { $count } jaar geleden
}

# Docs: collectie bewerken (EditCollectionModal)
docs-edit-collection-title = Collectie bewerken
docs-edit-collection-name = Naam
docs-edit-collection-slug = Slug
docs-edit-collection-slug-help = URL-fragment voor deze collectie. Alleen kleine letters, cijfers en streepjes.
docs-edit-collection-icon = Pictogram
docs-edit-collection-color = Kleur
docs-edit-collection-description = Korte omschrijving
docs-edit-collection-description-placeholder = Optionele tagline boven het overzicht van de collectie
docs-edit-collection-description-help = Het volledige overzicht bewerk je rechtstreeks op de landingspagina van de collectie.
docs-edit-collection-hide-titles-aria = Paginatitels verbergen voor niet-leden
docs-edit-collection-hide-titles-label = Paginatitels verbergen voor niet-leden
docs-edit-collection-hide-titles-help = Wikilinks tussen collecties tonen "Beperkte pagina" voor lezers zonder toegang, in plaats van de titel te lekken. Aanbevolen voor gevoelige collecties.
docs-edit-collection-name-required = Naam is verplicht.
docs-edit-collection-save-error = Opslaan mislukt. Probeer het opnieuw.
docs-edit-collection-cancel = Annuleren
docs-edit-collection-save = Wijzigingen opslaan
docs-edit-collection-saving = Opslaan...

# Docs: document verplaatsen (MoveDocumentModal)
docs-move-title = Document verplaatsen
docs-move-search-placeholder = Pagina's zoeken...
docs-move-root-label = Hoofdniveau (geen bovenliggende)
docs-move-current-badge = Huidig
docs-move-empty-search = Geen overeenkomende pagina's gevonden.
docs-move-empty = Geen pagina's beschikbaar.
docs-move-cancel = Annuleren
docs-move-action = Verplaatsen
docs-move-moving = Verplaatsen...

# Docs: paginarechten (PagePermissionsModal)
docs-page-permissions-title = Paginarechten
docs-page-permissions-mode-inherit = Overnemen van collecties
docs-page-permissions-mode-custom = Aangepaste toegang
docs-page-permissions-inherit-description = Deze pagina erft zichtbaarheid van zijn collecties. Gebruikers met toegang tot een van de collecties zien deze pagina.
docs-page-permissions-no-collections = In geen enkele collectie, zichtbaar voor iedereen.
docs-page-permissions-custom-description = Selecteer welke groepen en gebruikers toegang hebben tot deze pagina. Dit overschrijft de rechten op collectieniveau.
docs-page-permissions-picker-placeholder = Zoek gebruikers en groepen...
docs-page-permissions-no-selection-warning = Geen groepen of gebruikers geselecteerd, niemand behalve beheerders kan deze pagina zien.
docs-page-permissions-cancel = Annuleren
docs-page-permissions-save = Opslaan
docs-page-permissions-saving = Opslaan...

# Docs: gekoppelde tickets (PageTicketLinksPanel)
docs-page-tickets-heading = Gekoppelde tickets
docs-page-tickets-add = Ticket koppelen
docs-page-tickets-loading = Laden...
docs-page-tickets-empty = Nog geen tickets gekoppeld aan deze pagina.
docs-page-tickets-resolved-heading = Opgelost
docs-page-tickets-referenced-heading = Verwezen
docs-page-tickets-fallback-title = Ticket #{ $id }
docs-page-tickets-unlink = Ticket #{ $id } ontkoppelen

# Docs: auteursbadge (DocumentAuthorBadge)
docs-author-badge-fallback-name = Onbekend
docs-author-badge-verifier-fallback = Iemand
docs-author-badge-title-verified = Geschreven door { $author } · geverifieerd { $relative }
docs-author-badge-title-basic = Geschreven door { $author }
docs-author-badge-popover-aria = Auteur en verificatie van het document
docs-author-badge-created = Aangemaakt
docs-author-badge-author = Auteur
docs-author-badge-last-edited-by = Laatst bewerkt door
docs-author-badge-verification = Verificatie
docs-author-badge-state-verified = Geverifieerd
docs-author-badge-state-stale = Verouderd
docs-author-badge-state-never = Niet geverifieerd
docs-author-badge-last-verified = Laatst geverifieerd
docs-author-badge-verify-prompt-never = Markeren als geverifieerd, opnieuw verifiëren elke:
docs-author-badge-verify-prompt-again = Opnieuw verifiëren, elke:
docs-author-badge-interval-30d = 30 d
docs-author-badge-interval-90d = 90 d
docs-author-badge-interval-180d = 180 d
docs-author-badge-interval-1y = 1 j
docs-author-badge-interval-never = Nooit
docs-author-badge-clear = Verificatie wissen
# Ticket: zijbalkvelden & afdrukkop (TicketDetails).
ticket-detail-title-label = Titel
ticket-detail-source-label = Bron
ticket-detail-source-tooltip = Geopend via { $provider }. Reacties worden via de thread teruggestuurd.
ticket-detail-source-email = E-mail
ticket-detail-source-slack = Slack
ticket-detail-source-teams = Microsoft Teams
ticket-detail-clear-requester = Aanvrager wissen
ticket-detail-add-requester = Aanvrager toevoegen
ticket-detail-find-user-placeholder = Zoek een gebruiker...
ticket-detail-assign-to-placeholder = Toewijzen aan...
ticket-detail-clear-assignee = Toewijzing wissen
ticket-detail-add-assignee = Toegewezene toevoegen
ticket-detail-claim = Claimen
ticket-detail-claim-title = Ken dit ticket aan jezelf toe
ticket-detail-sla-label = SLA
ticket-detail-sla-paused-target = doel { $target }
ticket-detail-scheduling-label = Planning
ticket-detail-scheduling-none = Geen
ticket-detail-scheduling-due-date = Vervaldatum
ticket-detail-scheduling-due-prefix = Vervalt { $date }
ticket-detail-scheduling-clear-due = Vervaldatum wissen
ticket-detail-scheduling-recurrence = Herhaling
ticket-detail-recurrence-none = Niet terugkerend
ticket-detail-recurrence-daily = Dagelijks
ticket-detail-recurrence-weekly = Wekelijks
ticket-detail-recurrence-weekdays = Werkdagen
ticket-detail-recurrence-monthly = Maandelijks
ticket-detail-recurrence-yearly = Jaarlijks
ticket-detail-recurrence-recurring = Terugkerend
ticket-detail-recurrence-custom-note = Aangepaste RRULE in gebruik ({ $rule }). Bewerk via de API.
ticket-detail-recurrence-respawn-note = Het sluiten van dit ticket maakt het volgende voorkomen aan.
ticket-detail-category-placeholder = Categorie selecteren...
ticket-detail-cycle-label = Cyclus
ticket-detail-cycle-tooltip = Cyclus { $name } ({ $state })
ticket-detail-resolution-label = Oplossing
ticket-detail-resolution-closed = Gesloten
ticket-detail-resolution-draft-from-notes = Concept op basis van notities
ticket-detail-resolution-draft-from-notes-title = { $count ->
    [one] { $count } interne notitie aan het oplossingsconcept toevoegen
   *[other] { $count } interne notities aan het oplossingsconcept toevoegen
  }
ticket-detail-resolution-placeholder = Wat heeft dit opgelost?
ticket-detail-audit-created = Aangemaakt
ticket-detail-audit-created-by = Aangemaakt door { $name }
ticket-detail-audit-modified = Bijgewerkt
ticket-detail-audit-closed = Gesloten
ticket-detail-audit-closed-by = Gesloten door { $name }
ticket-detail-print-status = Status
ticket-detail-print-priority = Prioriteit
ticket-detail-print-category = Categorie
ticket-detail-print-requester = Aanvrager
ticket-detail-print-assignee = Toegewezen aan
ticket-detail-print-created = Aangemaakt
ticket-detail-print-modified = Gewijzigd
ticket-detail-print-unassigned = Niet toegewezen
ticket-detail-print-unknown = Onbekend
ticket-detail-print-logo-alt = Logo
ticket-detail-print-qr-alt = QR-code van ticket
ticket-detail-print-qr-label = Scan om te openen

# Ticket: reacties en bijlagen (CommentsAndAttachments).
ticket-comments-section-title = Reacties en bijlagen
ticket-comments-drop-files = Bestanden hier neerzetten
ticket-comments-internal-banner = Alleen zichtbaar voor medewerkers. Niet verzonden via het kanaal van het ticket.
ticket-comments-placeholder-public = Voeg een nieuwe reactie toe...
ticket-comments-placeholder-internal = Notitie voor het team…
ticket-comments-record-voice = Spraaknotitie opnemen
ticket-comments-upload-file = Bestand uploaden
ticket-comments-visibility-group = Zichtbaarheid van reactie
ticket-comments-public-reply = Openbare reactie
ticket-comments-public-reply-title = Verzonden naar de aanvrager via het kanaal van het ticket
ticket-comments-internal-note = Interne notitie
ticket-comments-internal-note-title = Alleen zichtbaar voor agenten; niet doorgestuurd via het kanaal van het ticket
ticket-comments-submit-reply = Reactie versturen
ticket-comments-submit-note = Notitie toevoegen
ticket-comments-voice-note-filename = Spraaknotitie { $date }
ticket-comments-filter-group = Filter zichtbaarheid reacties
ticket-comments-filter-all = Alle ({ $count })
ticket-comments-filter-public = Openbaar ({ $count })
ticket-comments-filter-internal = Intern ({ $count })
ticket-comments-badge-internal = Intern
ticket-comments-badge-forwarded = Doorgestuurd
ticket-comments-badge-forwarded-title = Een agent heeft deze e-mail naar de helpdesk doorgestuurd
ticket-comments-action-download = Downloaden
ticket-comments-action-delete-comment = Reactie verwijderen
ticket-comments-action-delete-voice = Spraakbericht verwijderen
ticket-comments-audio-default = Audio
ticket-comments-audio-voice-message = Spraakbericht
ticket-comments-print-unknown-author = Onbekend
ticket-comments-show-quoted-thread = Geciteerde thread tonen
ticket-comments-show-quoted-reply = { $lines ->
    [one] Geciteerde reactie tonen ({ $lines } regel)
   *[other] Geciteerde reactie tonen ({ $lines } regels)
  }
ticket-comments-show-original = Oorspronkelijk bericht tonen
ticket-comments-show-original-title = Open de ruwe RFC-822-bron in een nieuw tabblad

# Ticket: activiteitenlogboek (TicketActivity).
ticket-activity-section-title = Activiteit
ticket-activity-load-error = Kan activiteit niet laden
ticket-activity-load-more-error = Kan meer activiteit niet laden
ticket-activity-empty = Nog geen activiteit.
ticket-activity-load-more = Oudere activiteit laden
ticket-activity-loading = Laden…
ticket-activity-actor-someone = Iemand
ticket-activity-actor-system = Systeem
ticket-activity-actor-sender = Afzender
ticket-activity-actor-email-aria = Afzender van e-mail
ticket-activity-actor-portal-aria = Inzending via openbaar portaal
ticket-activity-actor-portal-label = het openbare portaal
ticket-activity-channel-email = e-mail
ticket-activity-channel-slack = Slack
ticket-activity-channel-teams = Microsoft Teams
ticket-activity-channel-discord = Discord
ticket-activity-actor-title-subject = { $name } — Onderwerp: { $subject }
ticket-activity-actor-title-named = { $name } <{ $email }>
ticket-activity-actor-title-named-subject = { $name } <{ $email }> — Onderwerp: { $subject }
ticket-activity-to-assignee = aan { $name }
ticket-activity-phrase-created = heeft dit ticket aangemaakt
ticket-activity-phrase-opened-via = heeft dit ticket geopend via { $channel }
ticket-activity-phrase-submitted-via = heeft dit ticket ingediend via { $channel }
ticket-activity-phrase-deleted = heeft dit ticket verwijderd
ticket-activity-phrase-status-set = heeft de status ingesteld op { $name }
ticket-activity-phrase-status-changed = heeft de status gewijzigd
ticket-activity-phrase-reassigned = heeft dit ticket opnieuw toegewezen
ticket-activity-phrase-unassigned = heeft de toewijzing van dit ticket opgeheven
ticket-activity-phrase-priority-set = heeft de prioriteit ingesteld op { $priority }
ticket-activity-phrase-priority-changed = heeft de prioriteit gewijzigd
ticket-activity-phrase-renamed = heeft het ticket hernoemd naar "{ $title }"
ticket-activity-phrase-renamed-plain = heeft het ticket hernoemd
ticket-activity-phrase-category-changed = heeft de categorie gewijzigd
ticket-activity-phrase-verification-changed = heeft de verificatiestatus bijgewerkt
ticket-activity-phrase-tags-added = { $count ->
    [one] heeft een label toegevoegd
   *[other] heeft { $count } labels toegevoegd
  }
ticket-activity-phrase-tags-removed = { $count ->
    [one] heeft een label verwijderd
   *[other] heeft { $count } labels verwijderd
  }
ticket-activity-phrase-tags-updated = heeft de labels bijgewerkt
ticket-activity-phrase-resolution-changed = heeft de oplossingsnotities bijgewerkt
ticket-activity-phrase-watcher-self-start = volgt dit ticket nu
ticket-activity-phrase-watcher-self-auto = volgt dit ticket (automatisch geabonneerd bij eerste reactie)
ticket-activity-phrase-watcher-self-stop = volgt dit ticket niet meer
ticket-activity-phrase-watcher-added-named = heeft { $name } als volger toegevoegd
ticket-activity-phrase-watcher-added = heeft een volger toegevoegd
ticket-activity-phrase-watcher-removed-named = heeft { $name } als volger verwijderd
ticket-activity-phrase-watcher-removed = heeft een volger verwijderd
ticket-activity-phrase-updated = heeft het ticket bijgewerkt
ticket-activity-phrase-internal-note = heeft een interne notitie toegevoegd
ticket-activity-phrase-replied-via = heeft gereageerd via { $channel }
ticket-activity-phrase-comment-via = heeft een reactie toegevoegd via { $channel }
ticket-activity-phrase-commented = heeft op dit ticket gereageerd
ticket-activity-phrase-comment-deleted = heeft een reactie verwijderd
ticket-activity-phrase-generic = heeft een wijziging aangebracht

# Ticket: labelkiezer (TicketTagsField).
ticket-field-tags-label = Labels
ticket-field-tags-add = Label toevoegen
ticket-field-tags-remove = { $name } verwijderen
ticket-field-tags-picker-placeholder = Zoek of maak een label…
ticket-field-tags-loading = Laden…
ticket-field-tags-no-match = Geen overeenkomende labels.
ticket-field-tags-create = "{ $name }" aanmaken
ticket-field-tags-creating = Aanmaken…
ticket-field-tags-done = Klaar

# Ticket: volgers (TicketWatchersField).
ticket-field-watchers-label = Volgers
ticket-field-watchers-watching = Volgt
ticket-field-watchers-watch = Volgen
ticket-field-watchers-watch-title = Volg dit ticket voor updates
ticket-field-watchers-unwatch-title = Niet meer volgen
ticket-field-watchers-notify-internal = Melden bij interne notities
ticket-field-watchers-notify-internal-hint = Krijg een melding bij privéreacties van medewerkers
ticket-field-watchers-public-only = Alleen openbare reacties
ticket-field-watchers-prefs-title = Meldingsvoorkeuren
ticket-field-watchers-toggle-on = AAN
ticket-field-watchers-toggle-off = UIT
ticket-field-watchers-pref-load-error = Kan voorkeur niet laden
ticket-field-watchers-pref-save-error = Kan voorkeur niet opslaan
ticket-field-watchers-overflow-title = { $count ->
    [one] nog { $count }
   *[other] nog { $count }
  }

# Ticket: apparatenrij (TicketDevicesField).
ticket-field-devices-label = Activa
ticket-field-devices-add = Activum toevoegen
ticket-field-devices-detach = Activum loskoppelen
ticket-field-devices-fallback-name = Activum #{ $id }
ticket-field-devices-title-with-model = { $hostname } · { $model }

ticket-asset-usage-heading = Asset usage
ticket-asset-usage-empty-no-stock = No stock-tracked assets linked to this ticket.
ticket-asset-usage-empty-no-history = No usage recorded yet.
ticket-asset-usage-quantity-placeholder = Used (in { $unit })
ticket-asset-usage-notes-placeholder = Notes (optional)
ticket-asset-usage-load-failed = Failed to load usage history
ticket-asset-usage-record-failed = Failed to record usage

# Asset detail: usage history panel (AssetUsageHistory).
asset-usage-history-heading = Usage history
asset-usage-history-empty = No usage recorded yet.
asset-usage-history-load-failed = Failed to load usage history
asset-usage-history-load-more = Load more
asset-usage-history-loading = Loading…
asset-usage-history-ticket-link = Ticket #{ $id }
asset-usage-history-ad-hoc = Ad-hoc consumption

# Asset detail: ad-hoc consumption recording (AssetUsageHistory record form).
asset-usage-record-heading = Record consumption
asset-usage-record-on-hand = on hand
asset-usage-record-quantity-placeholder = Quantity ({ $unit })
asset-usage-record-notes-placeholder = Notes (optional)
asset-usage-record-submit = Record

# Phase H — restock affordance on the asset usage panel.
asset-usage-record-submit-usage-title = Record consumption (decrement stock)
asset-usage-record-submit-restock = + Restock

# Stock audit (physical count) on the asset history panel.
asset-audit-record-heading = Audit count
asset-audit-record-hint = enter the physical count, system corrects to match
asset-audit-record-placeholder = Counted ({ $unit })
asset-audit-record-notes-placeholder = Notes (optional)
asset-audit-record-submit = Save audit
asset-audit-record-failed = Failed to record audit
asset-audit-history-label = Audit
asset-audit-history-previous = was { $previous }
asset-usage-record-submit-restock-title = Record restock (increment stock)
asset-usage-record-failed = Failed to record usage

# Asset detail: stock tracking section + low-stock indicator (Phase G).
asset-detail-section-stock = Stock tracking
asset-detail-field-quantity = On-hand quantity
asset-detail-field-quantity-placeholder = e.g. 25
asset-detail-field-unit = Unit
asset-detail-field-unit-placeholder = e.g. m, L, pcs
asset-detail-field-low-stock-threshold = Low-stock threshold
asset-detail-field-low-stock-threshold-placeholder = e.g. 5
asset-detail-field-low-stock-threshold-help = Show a warning and broadcast an event when stock falls to or below this value.

# Editable kind / attributes affordance on existing assets.
asset-detail-attributes-save = Save attributes
asset-detail-attributes-discard = Discard
asset-detail-attributes-save-failed = Failed to save attributes
asset-detail-kind-change-confirm = Change kind to { $newKind }? Current attributes will be cleared, you can re-enter them against the new kind schema.
asset-detail-kind-change-failed = Failed to change kind
asset-detail-low-stock-warning = Low stock: { $quantity } { $unit } remaining (threshold { $threshold }).
asset-low-stock-toast-title = Low stock: { $name }
asset-low-stock-toast-body = { $quantity } { $unit } remaining (threshold { $threshold }).

# Asset list: low-stock badge surfaced on each row.
assets-list-low-stock-badge = Low stock
assets-list-low-stock-tooltip = { $quantity } { $unit } remaining (threshold { $threshold }).

# Ticket: gekoppelde tickets (TicketLinkedTicketsField).
ticket-field-linked-tickets-label = Gekoppelde tickets
ticket-field-linked-tickets-add = Ticket koppelen
ticket-field-linked-tickets-drop = Sleep hierheen om te koppelen

# Ticket: projecten (TicketProjectsField).
ticket-field-projects-label = Projecten
ticket-field-projects-add = Aan project toevoegen
ticket-field-projects-remove = Uit project verwijderen
ticket-field-projects-fallback = Project #{ $id }

# Ticket: gekoppelde documentatie (TicketLinkedDocs).
ticket-field-docs-label = Documentatie
ticket-field-docs-add = Opslaan als doc
ticket-field-docs-resolves-title = { $title } · lost dit ticket op

# Ticket: chips/badges.
ticket-chip-remove = { $label } verwijderen
ticket-chip-sidebar-remove = Verwijderen
ticket-chip-linked-ticket-fallback = Ticket #{ $id }
ticket-chip-linked-ticket-title = #{ $id } · { $title }
ticket-chip-unlink-ticket = Ticket loskoppelen
ticket-chip-gap-flagged = Gemarkeerd voor documentatie
ticket-chip-gap-view-queue = Bekijk in wachtrij →
ticket-chip-gap-remove-flag = Markering verwijderen
ticket-chip-preview-priority = Prioriteit
ticket-chip-preview-created = Aangemaakt
ticket-chip-preview-requester = Aanvrager
ticket-chip-preview-assignee = Toegewezen aan
ticket-chip-preview-unassigned = Niet toegewezen
ticket-chip-preview-unlink = Ticket loskoppelen
ticket-chip-device-warranty-active = Actief
ticket-chip-device-warranty-warning = Waarschuwing
ticket-chip-device-warranty-expired = Verlopen
ticket-chip-device-remove = Activum verwijderen
ticket-chip-device-view-title = Activum bekijken
ticket-chip-device-unnamed = Naamloos activum
ticket-chip-device-field-serial = Serienummer
ticket-chip-device-field-model = Model
ticket-chip-device-field-manufacturer = Fabrikant
ticket-chip-device-field-hostname = Hostnaam
ticket-chip-device-value-na = N.v.t.
ticket-chip-device-value-unknown = Onbekend
ticket-chip-device-copy-tooltip = Klik om te kopiëren
ticket-chip-device-copied = Gekopieerd!

# Ticket: status/prioriteit/categoriekiezer (CustomDropdown).
ticket-chip-dropdown-select = Selecteer...
ticket-chip-dropdown-status = Status selecteren
ticket-chip-dropdown-priority = Prioriteit selecteren
ticket-chip-dropdown-category = Categorie selecteren
ticket-chip-dropdown-option = Optie selecteren
# Beheer: omgevingsconfiguratie-melding (EnvConfigNotice)
admin-env-notice-title = Configuratie via omgevingsvariabelen
admin-env-notice-prefix = Instellingen worden geconfigureerd via omgevingsvariabelen in uw
admin-env-notice-suffix = bestand of Docker-omgeving.

# Beheer: systeeminformatie-kaart (SystemInfoCard)
admin-system-info-title = Systeeminformatie
admin-system-info-version = Versie
admin-system-info-environment = Omgeving
admin-system-info-uptime = Beschikbaarheid
admin-system-info-update-to = Bijwerken naar { $version }
admin-system-info-uptime-days = { $count } d
admin-system-info-uptime-hours = { $count } u
admin-system-info-uptime-minutes = { $count } m
admin-system-info-uptime-seconds = { $count } s

# Beheer: categorie-editor (CategoryEditPanel)
admin-categories-edit-title-edit = Categorie bewerken
admin-categories-edit-title-create = Categorie aanmaken
admin-categories-edit-delete-tooltip = Categorie verwijderen
admin-categories-edit-close-tooltip = Paneel sluiten
admin-categories-edit-name-label = Naam
admin-categories-edit-name-placeholder = Categorienaam invoeren
admin-categories-edit-description-label = Beschrijving
admin-categories-edit-description-placeholder = Optionele beschrijving
admin-categories-edit-icon-label = Pictogram
admin-categories-edit-icon-folder = Map
admin-categories-edit-icon-tag = Label
admin-categories-edit-icon-bug = Bug
admin-categories-edit-icon-settings = Instellingen
admin-categories-edit-icon-idea = Idee
admin-categories-edit-icon-question = Vraag
admin-categories-edit-icon-alert = Waarschuwing
admin-categories-edit-icon-star = Ster
admin-categories-edit-color-label = Kleur
admin-categories-edit-active-label = Actief
admin-categories-edit-visibility-label = Zichtbaar voor groepen
admin-categories-edit-visibility-hint = (leeg laten voor openbaar)
admin-categories-edit-visibility-toggle-aria = Zichtbaarheid voor { $name } wisselen
admin-categories-edit-member-count = { $count ->
    [one] { $count } lid
   *[other] { $count } leden
    }
admin-categories-edit-no-groups = Geen groepen beschikbaar. Maak eerst groepen aan.
admin-categories-edit-cancel = Annuleren
admin-categories-edit-save = Wijzigingen opslaan
admin-categories-edit-create = Categorie aanmaken

# Beheer: groepsconfiguratiepaneel (GroupConfigurationPanel)
admin-groups-config-subtitle = Groepsconfiguratie
admin-groups-config-delete-tooltip = Groep verwijderen
admin-groups-config-close-tooltip = Paneel sluiten
admin-groups-config-source-microsoft = Microsoft Entra ID
admin-groups-config-managed-by = Beheerd door { $source }
admin-groups-config-last-synced = Laatst gesynchroniseerd { $date }
admin-groups-config-unmanage = Beheer ontkoppelen
admin-groups-config-unmanage-processing = Bezig...
admin-groups-config-sync-settings = Synchronisatie-instellingen
admin-groups-config-general = Algemene informatie
admin-groups-config-name-label = Naam
admin-groups-config-name-placeholder = Groepsnaam invoeren
admin-groups-config-description-label = Beschrijving
admin-groups-config-description-placeholder = Optionele beschrijving
admin-groups-config-color-label = Kleur
admin-groups-config-save-changes = Wijzigingen opslaan
admin-groups-config-members = Leden
admin-groups-config-no-members = Geen leden
admin-groups-config-devices = Activa
admin-groups-config-device-sn = SN: { $sn }
admin-groups-config-no-devices = Geen activa
admin-groups-config-included-in = Opgenomen in
admin-groups-config-included-groups = Opgenomen groepen
admin-groups-config-includes-hint = Leden van opgenomen groepen worden behandeld als leden van deze groep voor zichtbaarheid, toegang en toewijzing.
admin-groups-config-source-direct = Direct
admin-groups-config-source-via = via
admin-groups-config-source-also-via = ook via
admin-groups-config-section-assigned = Toegewezen
admin-groups-config-section-included-via = Opgenomen via groepen
admin-groups-config-section-not-assigned = Niet toegewezen
admin-groups-config-search-users = Gebruikers zoeken...
admin-groups-config-search-devices = Activa zoeken op naam, hostnaam, serienummer...
admin-groups-config-search-groups = Groepen zoeken...
admin-groups-config-no-users-found = Geen gebruikers gevonden
admin-groups-config-no-devices-found = Geen activa gevonden
admin-groups-config-no-groups-found = Geen groepen gevonden
admin-groups-config-synced-badge = Gesynchroniseerd
admin-groups-config-synced-intune-tooltip = Gesynchroniseerd vanaf Microsoft Intune
admin-groups-config-selected-count = { $count } geselecteerd
admin-groups-config-member-count = { $count ->
    [one] { $count } lid
   *[other] { $count } leden
    }
admin-groups-config-save-members = Leden opslaan
admin-groups-config-save-devices = Activa opslaan
admin-groups-config-save-includes = Opgenomen groepen opslaan
admin-groups-config-not-found = Groep niet gevonden
admin-groups-config-cancel = Annuleren
admin-groups-config-delete-title = Groep verwijderen
admin-groups-config-delete-confirm = Groep verwijderen
admin-groups-config-delete-prompt-prefix = Weet je zeker dat je de groep
admin-groups-config-delete-prompt-suffix = wilt verwijderen? Alle lidassociaties worden verwijderd, maar de gebruikers blijven bestaan.
admin-groups-config-unmanage-title = Groep losmaken van beheer?
admin-groups-config-unmanage-title-named = { $name } losmaken van beheer?
admin-groups-config-unmanage-message = De groep wordt niet langer gesynchroniseerd met Microsoft Entra ID. Handmatige bewerkingen worden toegestaan, maar de bestaande synchronisatiegeschiedenis blijft behouden.
admin-groups-config-error-invalid-id = Ongeldige groeps-ID
admin-groups-config-error-load = Kan groepsgegevens niet laden
admin-groups-config-error-name-required = Groepsnaam is verplicht
admin-groups-config-error-save = Kan groep niet opslaan
admin-groups-config-error-members = Kan leden niet bijwerken
admin-groups-config-error-devices = Bijwerken van activa mislukt
admin-groups-config-error-includes = Kan opgenomen groepen niet bijwerken
admin-groups-config-error-delete = Kan groep niet verwijderen
admin-groups-config-error-unmanage = Kan beheer niet ontkoppelen
admin-groups-config-success-updated = Groep bijgewerkt
admin-groups-config-success-members = Leden bijgewerkt
admin-groups-config-success-devices = Activa succesvol bijgewerkt
admin-groups-config-success-includes = Opgenomen groepen bijgewerkt
admin-groups-config-success-unmanage = Groep wordt nu lokaal beheerd

# Ticketstabel-onderdelen (TicketsTable, TicketRow, TicketPreviewPane)
views-tickets-table-select-all-aria = Selecteer alle zichtbare tickets
views-tickets-table-resize-handle-tooltip = Sleep om te wijzigen · dubbelklik om passend te maken
views-ticket-row-select-aria = Ticket #{ $id } selecteren
views-ticket-row-recurring-tooltip = Terugkerend ticket
views-ticket-row-sla-badge = SLA
views-ticket-row-sla-breached-tooltip = SLA overschreden
views-ticket-row-sla-breached = Overschreden
views-ticket-row-sla-paused = Gepauzeerd
views-ticket-row-sla-on-track = Op schema
views-ticket-row-cycle-tooltip = Hoort bij een cyclus
views-ticket-row-cycle-label = cyclus #{ $id }
views-ticket-row-no-due-date = Geen vervaldatum
views-ticket-row-kb-badge = KB
views-ticket-row-kb-gap-tooltip = Kennishiaat-signaal: { $signal }
views-ticket-row-devices-count = { $count ->
    [one] { $count } apparaat
   *[other] { $count } apparaten
    }

# Ticket-voorbeeldpaneel (TicketPreviewPane)
views-ticket-preview-aria = Ticketvoorbeeld
views-ticket-preview-empty-title = Geen ticket geselecteerd
views-ticket-preview-empty-prefix = Klik op een rij of navigeer met
views-ticket-preview-empty-suffix = voor een voorbeeld.
views-ticket-preview-open = Openen
views-ticket-preview-close-tooltip = Voorbeeld sluiten (Esc)
views-ticket-preview-close-aria = Voorbeeld sluiten
views-ticket-preview-kb-gap = KB-hiaat
views-ticket-preview-recurring = Terugkerend
views-ticket-preview-properties = Eigenschappen
views-ticket-preview-assignee = Toegewezen aan
views-ticket-preview-requester = Aanvrager
views-ticket-preview-due-date = Vervaldatum
views-ticket-preview-not-set = Niet ingesteld
views-ticket-preview-cycle = Cyclus
views-ticket-preview-cycle-label = Cyclus #{ $id }
views-ticket-preview-category = Categorie
views-ticket-preview-sla = SLA
views-ticket-preview-activity = Activiteit
views-ticket-preview-last-activity = Laatste activiteit
views-ticket-preview-created = Aangemaakt
views-ticket-preview-affected-devices = Betrokken activa
views-ticket-preview-more-devices = { $count ->
    [one] +{ $count } meer
   *[other] +{ $count } meer
    }
views-ticket-preview-view-full = Beschrijving, opmerkingen en activa bekijken

# Ticket-heatmap (TicketHeatmap)
ticket-heatmap-title-closed = Afgesloten tickets
ticket-heatmap-title-activity = Ticketactiviteit
ticket-heatmap-error-load = Kan ticketgegevens niet laden. Probeer het opnieuw.
ticket-heatmap-tooltip-empty = Geen tickets
ticket-heatmap-tooltip-count = { $count ->
    [one] { $count } ticket
   *[other] { $count } tickets
    }
ticket-heatmap-day-sun = Zo
ticket-heatmap-day-mon = Ma
ticket-heatmap-day-tue = Di
ticket-heatmap-day-wed = Wo
ticket-heatmap-day-thu = Do
ticket-heatmap-day-fri = Vr
ticket-heatmap-day-sat = Za
ticket-heatmap-days-with-activity = { $count ->
    [one] { $count } dag met activiteit
   *[other] { $count } dagen met activiteit
    }
ticket-heatmap-legend-less = Minder
ticket-heatmap-legend-more = Meer

# Weergaven: filter toevoegen-menu (AddFilterMenu)
views-add-filter-trigger = Filter toevoegen
views-add-filter-back-tooltip = Terug (Backspace)
views-add-filter-search-title-placeholder = Titel zoeken…
views-add-filter-no-matches = Geen overeenkomstige waarden
views-add-filter-facet-title = Titel
views-add-filter-facet-status = Status
views-add-filter-facet-priority = Prioriteit
views-add-filter-facet-assignee = Toegewezen aan
views-add-filter-facet-sla = SLA
views-add-filter-facet-cycle = Cyclus

# Weergaven: filterwaardenlijst (FilterValueList)
views-filter-value-search-placeholder = Zoeken…
views-filter-value-no-matches = Geen resultaten
views-filter-value-no-options = Geen opties
views-filter-value-clear = Wissen

# Weergaven: weergavemenu (DisplayMenu)
views-display-menu-trigger = Weergave
views-display-menu-trigger-tooltip = Weergaveopties
views-display-menu-grouping = Groepering
views-display-menu-density = Dichtheid
views-display-menu-density-aria = Rijdichtheid
views-display-menu-density-compact = Compact
views-display-menu-density-cosy = Knus
views-display-menu-density-comfortable = Comfortabel
views-display-menu-group-none = Geen
views-display-menu-group-status = Status
views-display-menu-group-priority = Prioriteit
views-display-menu-group-assignee = Toegewezen aan
views-display-menu-group-sla = SLA
views-display-menu-group-cycle = Cyclus
views-display-menu-properties = Eigenschappen
views-display-menu-column-ticket-id = Ticket-nr.
views-display-menu-reset = Kolommen resetten
views-display-menu-reset-tooltip = Standaardvolgorde, breedte en zichtbaarheid van kolommen herstellen
views-display-menu-save-to-view = Opslaan in weergave

# Weergaven: tabbalken
views-tab-bar-aria = Weergave
views-project-tab-aria = Projectweergave
views-project-tab-board = Bord
views-project-tab-gantt = Gantt
views-project-tab-cycles = Cycli

# Weergaven: editor voor opgeslagen weergave (SavedViewEditorModal)
views-saved-editor-title = Weergave bewerken
views-save-as-title = Save view as
views-save-as-name-label = Name
views-save-as-save = Save
views-save-as-saving = Saving
views-save-as-cancel = Cancel
views-save-trigger = Save view as
views-save-default-suffix = (copy)
views-save-as-success = Saved view "{ $name }"
views-save-as-error = Could not save view
views-saved-editor-rename-error = Could not rename view
views-saved-editor-delete-error = Could not delete view
views-asset-switcher-placeholder = Asset views
views-user-switcher-placeholder = User views
views-saved-editor-name-label = Naam
views-saved-editor-delete = Weergave verwijderen
views-saved-editor-cancel = Annuleren
views-saved-editor-save = Opslaan
views-saved-editor-saving = Opslaan
views-saved-editor-confirm-title = Weergave verwijderen?
views-saved-editor-confirm-message = "{ $name }" verwijderen? Deze actie is onomkeerbaar, maak de weergave opnieuw aan als je hem nog nodig hebt.

# Weergaven: filterpil (FilterPill)
views-filter-pill-remove-tooltip = Filter { $label } verwijderen
views-filter-pill-search-title-placeholder = Titel zoeken…

# Weergaven: weergaveschakelaar (ViewSwitcher)
views-view-switcher-placeholder = Weergave
views-view-switcher-edit-view = Weergave bewerken…

# Dashboard: widget toegewezen tickets (UserAssignedTickets)
user-assigned-tickets-title-assigned = Toegewezen tickets
user-assigned-tickets-title-requested = Aangevraagde tickets
user-assigned-tickets-empty-title-assigned = Geen toegewezen tickets
user-assigned-tickets-empty-title-requested = Geen aangevraagde tickets
user-assigned-tickets-empty-current = Je bent helemaal bij!
user-assigned-tickets-error-assigned = Kan toegewezen tickets niet laden
user-assigned-tickets-error-requested = Kan aangevraagde tickets niet laden
user-assigned-tickets-status-active = Actief
user-assigned-tickets-status-active-desc = Open + In behandeling
user-assigned-tickets-status-open = Open
user-assigned-tickets-status-in-progress = In behandeling
user-assigned-tickets-status-closed = Afgesloten
user-assigned-tickets-status-all = Alle
user-assigned-tickets-status-all-desc = Elke status
user-assigned-tickets-status-filter-aria = Statusfilter { $title }
user-assigned-tickets-sort-priority = Prioriteit
user-assigned-tickets-sort-priority-desc = Prioriteit, daarna recent
user-assigned-tickets-sort-recent = Recent
user-assigned-tickets-sort-recent-desc = Recent gewijzigd
user-assigned-tickets-sort-oldest = Oudste
user-assigned-tickets-sort-oldest-desc = Oudste eerst, voor triage
user-assigned-tickets-filter-high-priority = Alleen hoge prioriteit
user-assigned-tickets-filter-new-activity = Alleen nieuwe activiteit

# Dashboard: widget recente tickets (RecentTickets)
recent-tickets-empty = Geen recente tickets
recent-tickets-context-open-new-tab = Openen in nieuw tabblad
recent-tickets-context-copy-link = Link kopiëren
recent-tickets-context-remove = Verwijderen uit recent

# Plugin admin: lifecycle state pills (PluginStateBadge)
plugin-state-active = Actief
plugin-state-disabled = Uitgeschakeld
plugin-state-quarantined = In quarantaine
plugin-state-uninstalled = Verwijderd

# Plugin admin: trust tier pills (PluginTrustBadge)
plugin-trust-official = Officieel
plugin-trust-verified = Geverifieerd
plugin-trust-community = Community
plugin-trust-local = Lokaal

# Plugin admin: row card (PluginCard)
plugin-card-installed-on = Geïnstalleerd { $date }
plugin-card-permissions = { $count ->
    [one] { $count } machtiging
   *[other] { $count } machtigingen
  }
plugin-card-sr-plugin-name = Pluginnaam
plugin-card-sr-installed = Geïnstalleerd
plugin-card-sr-permission-count = Aantal machtigingen

# Plugin admin: detail view (PluginDetailView)
plugin-detail-back = Terug naar plugins
plugin-detail-loading = Plugin laden...
plugin-detail-loading-settings = Instellingen laden...
plugin-detail-lifecycle-heading = Levenscyclus
plugin-detail-settings-heading = Instellingen
plugin-detail-metadata-heading = Metadata
plugin-detail-metadata-source = Bron
plugin-detail-metadata-permissions = Machtigingen
plugin-detail-metadata-permissions-count = { $count ->
    [one] { $count } gedeclareerd
   *[other] { $count } gedeclareerd
  }
plugin-detail-metadata-repository = Repository
plugin-detail-required-aria = verplicht
plugin-detail-secret-configured = Geconfigureerd
plugin-detail-secret-update = Bijwerken
plugin-detail-secret-cancel = Annuleren
plugin-detail-secret-placeholder = Waarde invoeren
plugin-detail-secret-placeholder-new = Nieuwe waarde invoeren
plugin-detail-boolean-enabled = Ingeschakeld
plugin-detail-action-enable = Inschakelen
plugin-detail-action-disable = Uitschakelen
plugin-detail-action-uninstall = Verwijderen
plugin-detail-action-discard = Verwerpen
plugin-detail-action-save = Wijzigingen opslaan
plugin-detail-action-saving = Opslaan...
plugin-detail-status-missing-required = { $count ->
    [one] { $count } verplicht veld ontbreekt
   *[other] { $count } verplichte velden ontbreken
  }
plugin-detail-status-unsaved = { $count ->
    [one] { $count } niet-opgeslagen wijziging
   *[other] { $count } niet-opgeslagen wijzigingen
  }
plugin-detail-status-all-saved = Alle wijzigingen opgeslagen
plugin-detail-toast-saved = { $count ->
    [one] Instelling opgeslagen
   *[other] Instellingen opgeslagen
  }
plugin-detail-toast-enabled = Plugin ingeschakeld
plugin-detail-toast-disabled = Plugin uitgeschakeld
plugin-detail-error-load = Plugin laden mislukt
plugin-detail-error-save = Instellingen opslaan mislukt. Probeer het opnieuw.
plugin-detail-error-toggle = Plugin schakelen mislukt
plugin-detail-error-uninstall = Plugin verwijderen mislukt
plugin-detail-uninstall-title = Plugin verwijderen
plugin-detail-uninstall-prompt-prefix = Verwijderen
plugin-detail-uninstall-prompt-mid = ? Het
plugin-detail-uninstall-prompt-suffix = -beleid van de plugin bepaalt of de gegevens behouden of verwijderd worden.
plugin-detail-uninstall-cancel = Annuleren
plugin-detail-uninstall-confirm = Verwijderen

# Plugin admin: sideload view (PluginSideloadView)
plugin-sideload-back = Terug naar plugins
plugin-sideload-title = Ondertekende zip handmatig laden
plugin-sideload-intro-prefix = Voor plugins die nog niet in het register staan. Het bundel moet ondertekend zijn door een geregistreerde uitgever of door de lokale ondertekensleutel van deze instantie; niet-ondertekende uploads worden geweigerd. Op zoek naar een officiële plugin? Bekijk eerst het
plugin-sideload-intro-link = register
plugin-sideload-intro-suffix = .
plugin-sideload-dropzone-aria = Kies pluginzipbestand
plugin-sideload-choose-different = Kies een ander bestand
plugin-sideload-drop-here = Sleep hier je pluginzip
plugin-sideload-or-browse = of klik om te bladeren
plugin-sideload-warning-title = Laad alleen plugins van bronnen die je vertrouwt.
plugin-sideload-warning-prefix = Een handtekening bevestigt dat het bundel na ondertekening niet is gewijzigd, maar staat niet in voor de bedoelingen van de uitgever. Een geïnstalleerde plugin draait in de beheerinterface met toegang tot je sessie. Geef de voorkeur aan het
plugin-sideload-warning-link = register
plugin-sideload-warning-suffix = voor gecontroleerde uitgevers, en controleer de broncode van alles wat je handmatig laadt.
plugin-sideload-cancel = Annuleren
plugin-sideload-install = Plugin installeren
plugin-sideload-installing = Installeren...
plugin-sideload-error-not-zip = Selecteer een .zip-bestand
plugin-sideload-error-too-large = Bestand moet kleiner zijn dan 2 MB
plugin-sideload-error-install-failed = Plugin installeren mislukt
# Gast- en publieke weergaven (GuestTicketSubmitView, GuestTicketStatusView,
# PublicDocsView, PublicDocView, HelpView, PublicLayout,
# FeatureDisabledNotice). Tekst voor niet-aangemelde bezoekers.

# Gedeeld: melding uitgeschakelde functie en publieke layout
feature-disabled-sign-in = Inloggen
public-layout-home-aria = { $appName } home
public-layout-logo-aria = { $appName }-logo
public-layout-nav-aria = Publieke navigatie
public-layout-docs-link = Documentatie
public-layout-help-link = Hulp

# Ticket indienen als gast
guest-submit-disabled-title = Ticket indienen is niet beschikbaar
guest-submit-disabled-message = Tickets indienen als gast is momenteel uitgeschakeld. Log in als je een account hebt.
guest-submit-verify-title = Controleer je inbox
guest-submit-verify-message-prefix = Klik op de bevestigingslink die we hebben gestuurd naar
guest-submit-verify-message-suffix = om je ticket vrij te geven en je portaal in te stellen.
guest-submit-verify-spam-hint = Niet ontvangen? Controleer je spam en probeer het over een paar minuten opnieuw.
guest-submit-another = Nog een ticket indienen
guest-submit-success-title = Ticket ontvangen
guest-submit-success-email-prefix = We hebben een bevestiging gestuurd naar
guest-submit-success-email-suffix = met een link om in te loggen en de voortgang te volgen.
guest-submit-success-no-email = Je ticket is geregistreerd. Ons team neemt per e-mail contact op.
guest-submit-success-reference-prefix = Referentienummer
guest-submit-track-heading = Volgen zonder in te loggen
guest-submit-copied = Gekopieerd
guest-submit-copy = Kopiëren
guest-submit-track-hint = Bewaar deze link, het is de enige manier om het ticket te bekijken zonder in te loggen.
guest-submit-view-status = Ticketstatus bekijken
guest-submit-another-short = Nog een indienen
guest-submit-heading = Ticket indienen
guest-submit-tagline = We nemen per e-mail contact op.
guest-submit-honeypot-label = Website
guest-submit-field-name = Je naam
guest-submit-field-name-placeholder = Jan Jansen
guest-submit-field-email = E-mailadres
guest-submit-field-email-placeholder = jij@voorbeeld.nl
guest-submit-field-title = Onderwerp
guest-submit-field-title-placeholder = Een korte samenvatting van wat je nodig hebt
guest-submit-field-description = Omschrijving
guest-submit-field-description-placeholder = Vertel ons wat er aan de hand is en hoe we kunnen helpen.
guest-submit-description-counter = { $count } / 10000
guest-submit-attachments-label = Bijlagen
guest-submit-attachments-optional = (optioneel)
guest-submit-attachments-counter = { $count } / { $max }
guest-submit-attachments-uploading = Uploaden...
guest-submit-attachments-pick = Klik om een bestand toe te voegen
guest-submit-attachments-hint = Afbeeldingen, PDF of tekst. Maximaal { $size } MB per stuk.
guest-submit-attachments-remove-aria = { $name } verwijderen
guest-submit-submitting = Versturen...
guest-submit-submit = Ticket indienen
guest-submit-have-account = Heb je al een account?
guest-submit-sign-in = Inloggen
guest-submit-error-name = Vul je naam in.
guest-submit-error-email = Vul een geldig e-mailadres in.
guest-submit-error-title = Vul een onderwerp in.
guest-submit-error-description = Beschrijf het probleem.
guest-submit-error-uploads-pending = Wacht tot het uploaden van bestanden klaar is.
guest-submit-error-rate-limited = Te veel inzendingen vanuit je netwerk. Probeer het later opnieuw.
guest-submit-error-disabled = Tickets indienen is uitgeschakeld.
guest-submit-error-account-exists = Er bestaat al een account voor dit e-mailadres. Log in om een ticket in te dienen.
guest-submit-error-generic = Indienen van het ticket is mislukt. Probeer het opnieuw.
guest-submit-error-network = Netwerkfout. Probeer het opnieuw.
guest-submit-attach-error-max = Maximaal { $max } bijlagen.
guest-submit-attach-error-too-large = { $name } is groter dan { $size } MB.
guest-submit-attach-error-rate-limited = Te veel uploads vanuit je netwerk. Probeer het later opnieuw.
guest-submit-attach-error-too-large-server = { $name } is te groot.
guest-submit-attach-error-disabled = Bijlagen worden op dit moment niet geaccepteerd.
guest-submit-attach-error-generic = Upload mislukt. Probeer het opnieuw.
guest-submit-attach-error-network = Netwerkfout bij het uploaden van het bestand.
guest-submit-size-bytes = { $bytes } B
guest-submit-size-kb = { $value } kB
guest-submit-size-mb = { $value } MB

# Gast ticketstatus
guest-status-loading-aria = Ticket laden
guest-status-disabled-title = Status opzoeken is niet beschikbaar
guest-status-disabled-message = Het opzoeken van de ticketstatus voor gasten is momenteel uitgeschakeld.
guest-status-ticket-number = Ticket #{ $id }
guest-status-priority = Prioriteit
guest-status-opened = Geopend
guest-status-last-updated = Laatst bijgewerkt
guest-status-closed = Gesloten
guest-status-reply-prefix = Wil je reageren?
guest-status-reply-suffix = om een reactie toe te voegen.
guest-status-not-found-title = Ticket niet gevonden
guest-status-not-found-message = De link kan verlopen zijn of verkeerd zijn overgetypt.

# Lijst met publieke documentatie
public-docs-loading-aria = Documentatie laden
public-docs-disabled-title = Documentatie is niet beschikbaar
public-docs-disabled-message = Publieke documentatie is momenteel uitgeschakeld.
public-docs-heading = Documentatie
public-docs-tagline = Bekijk hulpartikelen en handleidingen.
public-docs-search-placeholder = Documentatie doorzoeken...
public-docs-search-aria = Documentatie doorzoeken
public-docs-no-results = Geen artikelen gevonden voor je zoekopdracht.
public-docs-empty = Er is nog geen documentatie beschikbaar.
public-docs-updated = Bijgewerkt { $date }

# Detail van publiek artikel
public-doc-loading-aria = Artikel laden
public-doc-back = Alle documenten
public-doc-last-updated = Laatst bijgewerkt op { $date }
public-doc-rich-text-prefix = Dit artikel maakt gebruik van collaboratieve rijke tekstbewerking. Hier zie je een vereenvoudigde weergave, voor de volledige ervaring met reacties en bijlagen moet je
public-doc-rich-text-link = inloggen
public-doc-rich-text-suffix = .
public-doc-not-found-title = Document niet gevonden
public-doc-not-found-message = Het is mogelijk verplaatst of privé gemaakt.
public-doc-back-to-docs = Terug naar documenten

# Hulppagina
help-disabled-title = Hulppagina is niet beschikbaar
help-disabled-message = De zelfbediening-hulppagina is momenteel uitgeschakeld.
help-heading = Waarmee kunnen we helpen?
help-tagline = Hier zijn een paar dingen die je zonder account kunt doen.
help-card-submit-title = Ticket indienen
help-card-submit-desc = Meld een probleem en we nemen per e-mail contact op.
help-card-docs-title = Documentatie bekijken
help-card-docs-desc = Publieke artikelen, handleidingen en how-tos.
help-card-reset-title = Wachtwoord opnieuw instellen
help-card-reset-desc = Geen toegang meer tot je account? Begin hier.
help-card-signin-title = Inloggen
help-card-signin-desc = Heb je al een account?

# Settings: appearance pane (AppearanceSettings).
settings-appearance-title = Weergave
settings-appearance-theme-heading = Thema
settings-appearance-theme-description = Kies je voorkeurskleurenschema
settings-appearance-device-local-label = Thema alleen voor dit apparaat
settings-appearance-device-local-description = Synchroniseer het thema niet tussen apparaten (gebruik bijvoorbeeld het E-Paper-thema op je tablet en houd de donkere modus op je laptop)
settings-appearance-section-automatic = Automatisch
settings-appearance-section-light = Lichte thema's
settings-appearance-section-dark = Donkere thema's
settings-appearance-red-horizon-easter-egg = Waarom zou je ze dit aandoen 😭
settings-appearance-accessibility-heading = Toegankelijkheid
settings-appearance-accessibility-description = Verbeter de leesbaarheid en het visuele onderscheid
settings-appearance-colorblind-label = Kleurenblindvriendelijke modus
settings-appearance-colorblind-description-monochrome = Altijd ingeschakeld voor monochromatische thema's zoals E-Paper en Red Horizon
settings-appearance-colorblind-description-default = Gebruik onderscheidende vormen voor statusindicatoren in plaats van alleen op kleur te vertrouwen
settings-appearance-display-heading = Weergave
settings-appearance-display-description = Pas indelingsvoorkeuren aan
settings-appearance-compact-label = Compacte weergave
settings-appearance-compact-description = Verminder de ruimte tussen elementen voor een compactere indeling
settings-appearance-theme-changed = Thema gewijzigd naar { $name }
settings-appearance-theme-changed-device-only = Thema gewijzigd naar { $name } (alleen op dit apparaat)
settings-appearance-theme-save-failed = Kan themavoorkeur niet opslaan
settings-appearance-colorblind-toggled = Kleurenblindvriendelijke modus { $state ->
    [enabled] ingeschakeld
   *[disabled] uitgeschakeld
}
settings-appearance-device-local-toggled = Thema alleen voor dit apparaat { $state ->
    [enabled] ingeschakeld
   *[disabled] uitgeschakeld
}
settings-appearance-compact-toggled = Compacte weergave { $state ->
    [enabled] ingeschakeld
   *[disabled] uitgeschakeld
}
settings-appearance-system-theme-name = Systeem

# Settings: ThemeCard.
settings-appearance-card-system-name = Systeem

# Settings: security pane (SecuritySettings).
settings-security-title = Wachtwoord
settings-security-label-current = Huidig wachtwoord
settings-security-label-new = Nieuw wachtwoord
settings-security-label-confirm = Bevestig nieuw wachtwoord
settings-security-placeholder-current = Voer je huidige wachtwoord in
settings-security-placeholder-new = Voer je nieuwe wachtwoord in
settings-security-placeholder-confirm = Bevestig je nieuwe wachtwoord
settings-security-placeholder-admin-new = Voer nieuw wachtwoord in
settings-security-placeholder-admin-confirm = Bevestig nieuw wachtwoord
settings-security-hint-length = Het wachtwoord moet minimaal 8 tekens lang zijn
settings-security-error-mismatch = Wachtwoorden komen niet overeen
settings-security-submit-change = Wachtwoord wijzigen
settings-security-submit-reset = Wachtwoord opnieuw instellen
settings-security-error-form-invalid = Vul alle velden correct in
settings-security-success-changed = Wachtwoord succesvol gewijzigd
settings-security-error-change-failed = Wachtwoord wijzigen mislukt. Controleer je huidige wachtwoord.
settings-security-success-reset = Wachtwoord opnieuw ingesteld voor deze gebruiker
settings-security-error-reset-failed = Wachtwoord opnieuw instellen mislukt

# OAuth callback view + card.
auth-callback-loading-default = Aanmelding voltooien...
auth-callback-loading-processing = Authenticatie verwerken...
auth-callback-loading-success = Gelukt! Doorverwijzen...
auth-callback-loading-subtitle = Even geduld terwijl we de authenticatie voltooien
auth-callback-success-title = Authenticatie geslaagd
auth-callback-success-subtitle = Doorverwijzen...
auth-callback-technical-details = Technische details
auth-callback-provider-microsoft = Microsoft
auth-callback-provider-sso = SSO
auth-callback-error-missing-params = Vereiste authenticatieparameters ontbreken
auth-callback-error-missing-detail = Ontbreekt: { $fields }
auth-callback-error-missing-field-code = code
auth-callback-error-missing-field-state = state
auth-callback-error-invalid-response = Ongeldige reactie van de server
auth-callback-error-no-response = Geen reactie van de server ontvangen
auth-callback-error-unknown = Onbekende fout
auth-callback-error-generic-message = Er is een onverwachte fout opgetreden tijdens de authenticatie
auth-callback-error-status-prefix = Status: { $status }
auth-callback-already-title = Account al gekoppeld
auth-callback-already-message = Dit { $provider }-account is al gekoppeld aan een andere gebruiker in het systeem.
auth-callback-already-suggestion-microsoft = Probeer in te loggen met een ander { $provider }-account of neem contact op met je beheerder.
auth-callback-already-suggestion-generic = Probeer in te loggen met een ander account of neem contact op met je beheerder.
auth-callback-invalid-title = Authenticatie mislukt
auth-callback-invalid-message = Het authenticatieverzoek was ongeldig of verlopen.
auth-callback-invalid-suggestion-microsoft = Probeer je { $provider }-account opnieuw te koppelen.
auth-callback-invalid-suggestion-generic = Probeer opnieuw in te loggen.
auth-callback-generic-title = Authenticatie mislukt
auth-callback-generic-suggestion = Probeer het opnieuw of neem contact op met support als het probleem aanhoudt.
auth-callback-action-try-different = Probeer een ander account
auth-callback-action-back-settings = Terug naar instellingen
auth-callback-action-return-login = Terug naar inloggen
auth-callback-action-try-again = Opnieuw proberen

# Dashboard-widgets
dashboard-widget-shell-action-view-all = Alles tonen
dashboard-widget-shell-empty-title-default = Nog niets om te tonen.
dashboard-widget-shell-drag-label = { $title } verslepen
dashboard-widget-shell-size-group-label = Grootte van { $title }
dashboard-widget-shell-size-option-title = Grootte { $size } van 3
dashboard-widget-shell-hide-label = { $title } verbergen
dashboard-widget-shell-loading-label = { $title } wordt geladen

dashboard-edit-bar-editing = Dashboard bewerken
dashboard-edit-bar-add-widget = Widget toevoegen
dashboard-edit-bar-reset = Herstellen
dashboard-edit-bar-done = Klaar
dashboard-edit-bar-reset-confirm-title = Dashboardindeling herstellen?
dashboard-edit-bar-reset-confirm-message = Je aangepaste indeling wordt vervangen door de standaardindeling voor jouw rol.
dashboard-edit-bar-reset-confirm-label = Herstellen

dashboard-add-widget-title = Widget toevoegen
dashboard-add-widget-all-added = Alle beschikbare widgets staan al op je dashboard.

dashboard-staff-queue-title = Wachtrij
dashboard-staff-queue-configure-aria = Wachtrijstatistieken configureren
dashboard-staff-queue-configure-title = Statistieken configureren
dashboard-staff-queue-error = Kan wachtrijstatistieken niet laden
dashboard-staff-queue-metric-unassigned-label = Niet toegewezen
dashboard-staff-queue-metric-unassigned-desc = Open, zonder behandelaar
dashboard-staff-queue-metric-all-label = Alle tickets
dashboard-staff-queue-metric-all-desc = Alle statussen
dashboard-staff-queue-metric-open-label = Open
dashboard-staff-queue-metric-open-desc = Status: open
dashboard-staff-queue-metric-in-progress-label = In behandeling
dashboard-staff-queue-metric-in-progress-desc = Wordt nu opgepakt
dashboard-staff-queue-metric-high-priority-label = Hoge prioriteit
dashboard-staff-queue-metric-high-priority-desc = Hoge prioriteit, nog open
dashboard-staff-queue-metric-closed-today-label = Vandaag gesloten
dashboard-staff-queue-metric-closed-today-desc = Gesloten in de laatste 24 uur

dashboard-staff-yours-title = Van jou
dashboard-staff-yours-error = Kan tellingen niet laden
dashboard-staff-yours-assigned = Toegewezen
dashboard-staff-yours-open = Open
dashboard-staff-yours-in-progress = In behandeling
dashboard-staff-yours-closed = Gesloten

dashboard-user-summary-title = Overzicht
dashboard-user-summary-error = Kan overzicht niet laden
dashboard-user-summary-requests = Aanvragen
dashboard-user-summary-open = Open
dashboard-user-summary-in-progress = In behandeling
dashboard-user-summary-resolved = Opgelost

dashboard-queue-metrics-picker-title = Wachtrijstatistieken configureren
dashboard-queue-metrics-picker-hint = Kies maximaal { $max } statistieken voor de wachtrijkaart.
dashboard-queue-metrics-picker-count = ({ $count } / { $max } gekozen)
dashboard-queue-metrics-picker-toggle-aria = { $label } omschakelen
dashboard-queue-metrics-picker-cancel = Annuleren
dashboard-queue-metrics-picker-save = Opslaan

dashboard-knowledge-gaps-title = Kennislacunes
dashboard-knowledge-gaps-title-with-count = Kennislacunes ({ $count })
dashboard-knowledge-gaps-action = Wachtrij bekijken
dashboard-knowledge-gaps-error = Kan lacunes niet laden
dashboard-knowledge-gaps-empty-title = Geen openstaande lacunes
dashboard-knowledge-gaps-empty-description = Tickets die zijn gemarkeerd voor documentatie verschijnen hier.
dashboard-knowledge-gaps-signal-count =
    { $count ->
        [one] 1 signaal
       *[other] { $count } signalen
    }
dashboard-knowledge-gaps-impact-tickets = { $count } tickets
dashboard-knowledge-gaps-impact-searches = { $count } zoekopdrachten
dashboard-knowledge-gaps-impact-tooltip-tickets = { $count } tickets die vraag naar dit document tonen
dashboard-knowledge-gaps-impact-tooltip-searches = { $count } zoekopdrachten die vraag naar dit document tonen

dashboard-channel-health-title = Kanaalstatus
dashboard-channel-health-action = Beheren
dashboard-channel-health-error = Kan kanalen niet laden
dashboard-channel-health-empty-title = Geen kanalen geconfigureerd
dashboard-channel-health-empty-description = Voeg een e-mailkanaal toe om tickets binnen te halen.
dashboard-channel-health-status-disabled = Uitgeschakeld
dashboard-channel-health-status-error = Fout
dashboard-channel-health-status-healthy = In orde
dashboard-channel-health-polled = opgehaald { $time }
dashboard-channel-health-never-polled = nog niet opgehaald

dashboard-my-assets-title = Mijn activa
dashboard-my-assets-error = Activa konden niet worden geladen
dashboard-my-assets-empty-title = Geen activa toegewezen
dashboard-my-assets-empty-description = Activa die aan je account zijn gekoppeld, verschijnen hier.
dashboard-my-assets-unknown-model = Onbekend model

dashboard-recently-viewed-title = Recent bekeken
dashboard-recently-viewed-error = Kan recent bekeken niet laden
dashboard-recently-viewed-empty-title = Nog niets om te tonen
dashboard-recently-viewed-empty-description = Tickets die je opent, verschijnen hier.

dashboard-starred-docs-title = Favoriete documenten
dashboard-starred-docs-error = Kan favoriete documenten niet laden
dashboard-starred-docs-empty-title = Geen favoriete pagina's
dashboard-starred-docs-empty-description = Markeer een document met een ster om het bij de hand te houden.

dashboard-unassigned-queue-title = Niet-toegewezen wachtrij
dashboard-unassigned-queue-error = Kan wachtrij niet laden
dashboard-unassigned-queue-empty-title = Lege inbox
dashboard-unassigned-queue-empty-description = Niets staat in de wachtrij.

# Basis-UI-laag
ui-site-header-untitled-ticket = Ticket zonder titel
ui-site-header-unknown-device = Onbekend apparaat
ui-site-header-ticket-title-placeholder = Tickettitel invoeren...
ui-site-header-document-title-placeholder = Documenttitel invoeren...
ui-site-header-create-aria = { $action } aanmaken
ui-site-header-inbox-tooltip = Postvak
ui-site-header-inbox-aria = Postvak openen
ui-user-selection-modal-title = Gebruiker toewijzen
ui-user-selection-modal-search-placeholder = Zoek op naam of e-mail...
ui-user-selection-modal-unassign = Gebruiker ontkoppelen
ui-user-selection-modal-error = Gebruikers laden mislukt
ui-user-selection-modal-empty-no-match = Geen gebruikers gevonden
ui-user-selection-modal-empty-no-users = Geen gebruikers beschikbaar
ui-user-selection-modal-role-admin = Beheerder
ui-user-selection-modal-role-technician = Agent
ui-user-selection-modal-role-user = Gebruiker
ui-user-card-role-admin = Beheerder
ui-user-card-role-technician = Agent
ui-user-card-role-user = Gebruiker
ui-quick-tooltip-unassigned = Niet toegewezen
ui-quick-tooltip-status-label = Status:
ui-quick-tooltip-requester-label = Aanvrager:
ui-quick-tooltip-assignee-label = Toegewezen aan:
ui-presence-stack-fallback-name = Iemand
ui-presence-stack-aria =
    { $count ->
        [one] { $count } persoon bekijkt
       *[other] { $count } personen bekijken
    }
ui-presence-stack-overflow-title =
    { $count ->
        [one] { $count } andere bekijkt
       *[other] { $count } anderen bekijken
    }
ui-status-badge-status-open = open
ui-status-badge-status-in-progress = in behandeling
ui-status-badge-status-closed = gesloten
ui-status-badge-priority-low = laag
ui-status-badge-priority-medium = gemiddeld
ui-status-badge-priority-high = hoog
ui-status-badge-priority-low-full = lage prioriteit
ui-status-badge-priority-medium-full = gemiddelde prioriteit
ui-status-badge-priority-high-full = hoge prioriteit
ui-heatmap-tooltip-more = ...en nog { $count ->
        [one] { $count } meer
       *[other] { $count } meer
    }
ui-device-groups-title = Groepen
ui-header-title-placeholder = Titel invoeren...

# Search + ticket remnants
search-global-filter-documentation = Documentatie
search-global-filter-tickets = Tickets
search-global-filter-devices = Activa
search-global-filter-users = Gebruikers
search-global-placeholder = Zoek tickets, documenten, activa, gebruikers
search-global-placeholder-filtered = Zoek in { $filter }
search-global-aria-label = Zoeken
search-global-prompt-title = Doorzoek je helpdesk
search-global-prompt-subtitle = Vind tickets, documentatie, activa en meer
search-global-empty-prefix = Geen resultaten voor
search-global-empty-hint = Probeer andere zoektermen of controleer de spelling
search-global-hint-navigate = Navigeren
search-global-hint-open = Openen
search-global-hint-close = Sluiten
search-global-results-count =
    { $count ->
        [one] { $count } resultaat
       *[other] { $count } resultaten
    }
search-global-results-took = { $ms } ms
search-result-item-today = Vandaag
search-result-item-yesterday = Gisteren
search-result-item-days-ago = { $count } d geleden
search-result-item-weeks-ago = { $count } w geleden
search-result-item-months-ago = { $count } mnd geleden
search-result-item-years-ago = { $count } j geleden
search-result-item-internal-title = Interne notitie, alleen zichtbaar voor medewerkers
search-result-item-internal-badge = Intern
search-result-group-ticket = Tickets
search-result-group-comment = Reacties
search-result-group-documentation = Documentatie
search-result-group-attachment = Bijlagen
search-result-group-device = Activa
search-result-group-user = Gebruikers
tickets-cycle-burndown-load-error = Kan statistieken niet laden
tickets-cycle-burndown-cat-triage = Triage
tickets-cycle-burndown-cat-backlog = Backlog
tickets-cycle-burndown-cat-active = Actief
tickets-cycle-burndown-cat-in-review = In review
tickets-cycle-burndown-cat-done = Klaar
tickets-cycle-burndown-cat-cancelled = Geannuleerd
tickets-cycle-burndown-frozen = Bevroren
tickets-cycle-burndown-live = Live
tickets-cycle-burndown-loading = Laden...
tickets-cycle-burndown-tickets-done = Tickets klaar
tickets-cycle-burndown-complete = Voltooid
tickets-cycle-burndown-days-remaining =
    { $count ->
        [one] Dag te gaan
       *[other] Dagen te gaan
    }
tickets-cycle-burndown-snapshot-frozen = Snapshot bevroren { $date }
tickets-collaborative-article-title = Ticketnotities
tickets-collaborative-article-doc-title = Documentatie: ticket #{ $id }
tickets-collaborative-article-revision-history = Versiegeschiedenis
tickets-collaborative-article-convert-doc = Omzetten naar documentatiepagina
tickets-collaborative-article-open-full = Volledige editor openen
tickets-project-info-remove = Uit project verwijderen
tickets-project-info-description = Beschrijving
tickets-project-info-project-id = Project-ID
tickets-project-info-status = Status
tickets-project-info-tickets = Tickets
tickets-project-info-print-tickets =
    { $count ->
        [one] { $count } ticket
       *[other] { $count } tickets
    }
tickets-project-info-status-active = actief
tickets-project-info-status-completed = voltooid
tickets-project-info-status-archived = gearchiveerd
tickets-email-html-iframe-title = E-mailinhoud
tickets-email-html-show-less = Minder weergeven
tickets-email-html-show-full = Volledige e-mail weergeven
tickets-email-html-scaled = Aangepast aan ({ $pct }%)

# Header + nav polish
nav-logo-alt = Nosdesk-logo
nav-logo-alt-collapsed = Nosdesk
nav-section-recent-tickets = Recente tickets
nav-section-documentation = Documentatie
nav-more-heading = Meer navigatie
common-search-placeholder = Zoeken...
header-create-ticket = Nieuw ticket
header-create-project = Project aanmaken
header-add-ticket = Ticket toevoegen
header-create-user = Gebruiker aanmaken
header-create-asset = Activum aanmaken
header-create-document = Document aanmaken
nav-route-announcement = Genavigeerd naar { $title }
common-dropdown-select-placeholder = Selecteer een optie
common-dropdown-empty-message = Geen overeenkomsten

# Inbox + notifications polish
inbox-group-today = Vandaag
inbox-group-yesterday = Gisteren
inbox-group-this-week = Deze week
inbox-group-earlier = Eerder
inbox-mark-mentions-read = Vermeldingen als gelezen markeren
inbox-mark-all-read = Alles als gelezen markeren
inbox-empty-caught-up-title = Je bent helemaal bij
inbox-empty-caught-up-subtitle = Niets ongelezen op dit moment. Nieuwe meldingen verschijnen hier zodra ze binnenkomen.
inbox-empty-mentions-title = Nog geen vermeldingen
inbox-empty-mentions-subtitle = Als iemand je @vermeldt in een reactie, zie je dat hier.
inbox-empty-default-title = Nog geen meldingen
inbox-empty-default-subtitle = Updates van tickets, reacties, vermeldingen en documenten die je volgt komen hier binnen.
inbox-footer-loading-more = Meer laden...
inbox-footer-end-of-feed = Einde van de lijst
notifications-bell-header = Meldingen
notifications-bell-open-inbox = Inbox openen
notifications-bell-loading = Laden...
notifications-bell-load-more = Meer laden
notifications-bell-mark-mentions-read = Vermeldingen als gelezen markeren
notifications-bell-mark-all-read = Alles als gelezen markeren
notifications-bell-settings = Instellingen
notifications-filter-tabs-all = Alles
notifications-filter-tabs-unread = Ongelezen
notifications-filter-tabs-mentions = Vermeldingen
notifications-toast-new-with-title = Nieuwe melding: { $title } ({ $seq })
notifications-toast-new = Nieuwe melding ({ $seq })
# Route meta titles
route-title-login = Inloggen
route-title-reset-password = Wachtwoord herstellen
route-title-mfa-setup = MFA-instelling vereist
route-title-accept-invitation = Uitnodiging accepteren
route-title-onboarding = Instellen
route-title-guest-submit-ticket = Ticket indienen
route-title-guest-ticket-status = Ticketstatus
route-title-documentation = Documentatie
route-title-help = Hulp
route-title-dashboard = Dashboard
route-title-inbox = Inbox
route-title-tickets = Tickets
route-title-ticket-view = Ticket bekijken
route-title-ticket-notes = Notities ticket #{ $id }
route-title-user-profile = Gebruikersprofiel
route-title-user-settings = Gebruikersinstellingen
route-title-user-settings-profile = Instellingen gebruikersprofiel
route-title-user-settings-appearance = Instellingen weergave gebruiker
route-title-user-settings-notifications = Meldingsinstellingen gebruiker
route-title-user-settings-security = Beveiligingsinstellingen gebruiker
route-title-projects = Projecten
route-title-cycles = Cycli
route-title-cycle-detail = Cyclus
route-title-project-gantt = Gantt
route-title-assets = Middelen
route-title-asset-create = Activum aanmaken
route-title-asset-view = Activumdetails
route-title-asset-planner = Activaplanner
route-title-project-detail = Projectdetails
route-title-error = Fout
route-title-users = Gebruikers
route-title-documentation-drafts = Concepten
route-title-collection = Collectie
route-title-documentation-archived = Gearchiveerd
route-title-documentation-trash = Prullenbak
route-title-knowledge-gaps = Kennishiaten
route-title-profile = Profiel
route-title-profile-settings = Instellingen
route-title-profile-settings-profile = Profielinstellingen
route-title-profile-settings-appearance = Weergave-instellingen
route-title-profile-settings-notifications = Meldingsinstellingen
route-title-profile-settings-security = Beveiligingsinstellingen
route-title-administration = Beheer
route-title-admin-groups = Groepen
route-title-group-configuration = Groepsconfiguratie
route-title-admin-categories = Categorieën
route-title-admin-assignment-rules = Toewijzingsregels
route-title-admin-workflow = Werkstroom
route-title-admin-asset-kinds = Activatypen
route-title-admin-asset-kinds-new = Nieuw activatype
route-title-admin-asset-kinds-edit = Activatype bewerken
route-title-admin-api-tokens = API-tokens
route-title-admin-workspaces = Workspaces
route-title-admin-workspace-members = Workspace members
route-title-admin-canned-responses = Standaardantwoorden
route-title-admin-canned-responses-new = Nieuw standaardantwoord
route-title-admin-canned-responses-edit = Standaardantwoord bewerken
route-title-admin-webhooks = Webhooks
route-title-admin-sla = SLA
route-title-admin-plugins = Plug-ins
route-title-admin-plugin-registry = Plug-in-register
route-title-admin-plugin-sideload = Plug-in zijladen
route-title-admin-plugin-detail = Plug-indetails
route-title-admin-auth-providers = Authenticatieaanbieders
route-title-admin-search = Beheer zoekindex
route-title-admin-system-settings = Systeeminstellingen
route-title-admin-branding = Huisstijl
route-title-admin-audit-log = Auditlogboek
route-title-admin-email-queue = E-mailwachtrij
route-title-admin-email-suppressions = E-mailonderdrukkingen
route-title-admin-guest-access = Gasttoegang
route-title-admin-email-settings = E-mailconfiguratie
route-title-admin-channels-email = E-mailontvangst
route-title-admin-data-import = Gegevensimport
route-title-admin-microsoft-graph = Microsoft Graph-verbinding
route-title-admin-csv-import = CSV-import
route-title-admin-backup-restore = Back-up en herstel
route-title-group-detail = Groepsdetails
route-title-authenticating = Bezig met authenticeren...
route-title-pdf-viewer = PDF-viewer

# Loading modals + scattered polish
common-loading-projects = Projecten laden...
common-loading-devices = Activa laden...
common-loading-generic = Laden...
common-loading-groups = Groepen laden...
common-delete-item-aria = { $name } verwijderen
admin-branding-aria-logo = Logo
admin-branding-aria-logo-light = Logo voor licht thema
admin-branding-aria-favicon = Favicon

# Store error messages
error-store-workflow-states-load = Workflowstatussen konden niet worden geladen.
error-store-public-settings-load = Openbare instellingen konden niet worden geladen.
error-store-feature-flags-load = Functievlaggen konden niet worden geladen.
error-store-recent-tickets-load = Recente tickets konden niet worden opgehaald.
error-store-saved-views-load = Opgeslagen weergaven konden niet worden geladen.
error-store-saved-view-save = Weergave kon niet worden opgeslagen.
error-store-saved-view-update = Weergave kon niet worden bijgewerkt.
error-store-saved-view-delete = Weergave kon niet worden verwijderd.
error-store-cycles-load = Cycli konden niet worden geladen.
error-store-cycle-create = Cyclus kon niet worden aangemaakt.
error-store-cycle-update = Cyclus kon niet worden bijgewerkt.
error-store-cycle-complete = Cyclus kon niet worden voltooid.
error-store-cycle-archive = Cyclus kon niet worden gearchiveerd.
error-store-auth-profile-load = Je profiel kon niet worden geladen. Probeer het opnieuw.
error-store-auth-mfa-setup-start = MFA-instelling kon niet worden gestart. Probeer het opnieuw.
error-store-auth-mfa-setup-complete = MFA-instelling kon niet worden voltooid. Probeer het opnieuw.

# Q2 polish: helpers + registries + errors
priority-urgent = Urgent
priority-high = Hoog
priority-medium = Gemiddeld
priority-low = Laag
priority-none = Geen prioriteit
sla-breached = Overschreden
sla-at-risk = Risico
sla-on-track = Op schema
sla-paused = Gepauzeerd
sla-none = Geen SLA
status-open = Open
status-in-progress = In behandeling
status-closed = Gesloten
status-unknown = Onbekend
color-red = Rood
color-orange = Oranje
color-yellow = Geel
color-green = Groen
color-cyan = Cyaan
color-blue = Blauw
color-purple = Paars
color-pink = Roze
priority-indicator-low-aria = Lage prioriteit
priority-indicator-medium-aria = Gemiddelde prioriteit
priority-indicator-high-aria = Hoge prioriteit
priority-indicator-unknown-aria = Onbekende prioriteit
search-entity-type-tickets = Tickets
search-entity-type-comments = Reacties
search-entity-type-documentation = Documentatie
search-entity-type-attachments = Bijlagen
search-entity-type-devices = Activa
search-entity-type-users = Gebruikers
plugin-permission-ticket-read-label = Tickets lezen
plugin-permission-ticket-read-description = Ticketgegevens lezen
plugin-permission-ticket-write-label = Tickets schrijven
plugin-permission-ticket-write-description = Tickets aanmaken en bijwerken
plugin-permission-ticket-comment-label = Op tickets reageren
plugin-permission-ticket-comment-description = Reacties toevoegen aan tickets
plugin-permission-ticket-delete-label = Tickets verwijderen
plugin-permission-ticket-delete-description = Tickets verwijderen
plugin-permission-asset-read-label = Activa lezen
plugin-permission-asset-read-description = Activumgegevens lezen
plugin-permission-asset-write-label = Activa schrijven
plugin-permission-asset-write-description = Activa aanmaken en bijwerken
plugin-permission-user-read-label = Gebruikers lezen
plugin-permission-user-read-description = Profielgegevens van gebruikers lezen
plugin-permission-storage-plugin-label = Plugin-opslag
plugin-permission-storage-plugin-description = Sleutel-waarde-gegevens binnen de plugin opslaan
plugin-permission-collection-read-label = Collecties lezen
plugin-permission-collection-read-description = Records uit getypeerde collecties lezen
plugin-permission-collection-write-label = Collecties schrijven
plugin-permission-collection-write-description = Records in getypeerde collecties aanmaken en bijwerken
error-resource-not-found = De gevraagde bron is niet gevonden.
error-network = Kan geen verbinding maken met de server. Controleer je internetverbinding.
error-session-expired = Je sessie is verlopen. Log opnieuw in.
error-forbidden = Je hebt geen toestemming om deze actie uit te voeren.
plugin-error-load-failed = Plugin kon niet worden geladen
plugin-error-pending-review = Deze plugin wacht op beoordeling
plugin-error-not-installed = Plugin-component is niet geïnstalleerd
plugin-error-component-not-found = Component niet gevonden in plugin
plugin-error-timeout = De plugin deed er te lang over om te laden
plugin-error-failed = Plugin kon niet worden geladen
bulk-action-undo = Ongedaan maken
bulk-action-undone = Ongedaan gemaakt
bulk-action-undo-failed = Ongedaan maken mislukt
passkey-last-used-never = Nooit

# Dashboard widget registry
dashboard-widget-assigned-tickets-title = Toegewezen tickets
dashboard-widget-assigned-tickets-description = Je huidige werkwachtrij met status en prioriteit.
dashboard-widget-stats-yours-title = Jouw tellers
dashboard-widget-stats-yours-description = Snelle tellers van aan jou toegewezen tickets per status.
dashboard-widget-stats-queue-title = Wachtrijtellers
dashboard-widget-stats-queue-description = Niet-toegewezen en totale ticketaantallen in de wachtrij.
dashboard-widget-unassigned-queue-title = Niet-toegewezen wachtrij
dashboard-widget-unassigned-queue-description = Oudste open tickets zonder verantwoordelijke. Pak de volgende.
dashboard-widget-recently-viewed-title = Onlangs bekeken
dashboard-widget-recently-viewed-description = Tickets die je recent hebt bekeken.
dashboard-widget-starred-docs-title = Favoriete documenten
dashboard-widget-starred-docs-description = Documentatiepagina's die je als favoriet hebt gemarkeerd.
dashboard-widget-my-devices-title = Mijn activa
dashboard-widget-my-devices-description = Activa waaraan je als primaire gebruiker bent toegewezen.
dashboard-widget-channel-health-title = Kanaalstatus
dashboard-widget-channel-health-description = Status van inkomende e-mailkanalen, laatste poll, actief, fouten.
dashboard-widget-activity-heatmap-title = Activiteitsheatmap
dashboard-widget-activity-heatmap-description = 365-dagen heatmap van door jou gesloten tickets.
dashboard-widget-activity-heatmap-prop-title = Jouw activiteit
dashboard-widget-requested-tickets-title = Jouw verzoeken
dashboard-widget-requested-tickets-description = Tickets die je hebt geopend met huidige status.
dashboard-widget-requested-tickets-prop-title = Jouw verzoeken
dashboard-widget-stats-summary-title = Verzoeksamenvatting
dashboard-widget-stats-summary-description = Telling van jouw verzoeken per status.
dashboard-widget-knowledge-gaps-title = Kennisleemtes
dashboard-widget-knowledge-gaps-description = Belangrijkste documenten om te schrijven, gerangschikt op ticketbewijs.

dashboard-widget-sla-health-title = SLA-gezondheid
dashboard-widget-sla-health-description = Overzicht van tickets gedekt door een SLA-beleid.
dashboard-sla-health-title = SLA-gezondheid
dashboard-sla-health-action = SLA-beheer
dashboard-sla-health-tracked = Gevolgd
dashboard-sla-health-breached = Overschreden
dashboard-sla-health-at-risk = Risico
dashboard-sla-health-paused = Gepauzeerd
dashboard-sla-health-error = SLA-gezondheid kon niet worden geladen
dashboard-sla-health-empty-title = Geen tickets gevolgd
dashboard-sla-health-empty-description = Geen open tickets komen momenteel overeen met een SLA-beleid.
# Q1 polish: template attributes + static text
tickets-row-new-activity-tooltip = Nieuwe activiteit sinds je laatste bezoek
tickets-row-new-activity-aria = Nieuwe activiteit
common-confirm-delete-title = Verwijderen bevestigen
common-toast-dismiss = Sluiten
common-error-banner-dismiss = Foutmelding sluiten
common-route-progress-aria = Laden
common-bulk-actions-aria = Bulkacties
common-loading-more-aria = Meer laden
pagination-controls-page = Pagina
pagination-controls-show = Toon
pagination-controls-id-placeholder = ID
asset-modal-title = Selecteer een activum
asset-modal-search-placeholder = Activa zoeken op naam, hostnaam, serienummer, fabrikant of gebruiker...
asset-modal-owner = Eigenaar
asset-modal-unassigned = Niet toegewezen
asset-modal-col-device = Activum
asset-modal-col-status = Status
asset-modal-col-serial = Serie
asset-modal-col-user = Gebruiker
project-modal-title = Aan project toevoegen
project-modal-search-placeholder = Zoek projecten op naam of beschrijving...
project-modal-col-name = Projectnaam
project-modal-col-description = Beschrijving
project-modal-col-status = Status
project-modal-col-tickets = Tickets
project-modal-col-action = Actie
kanban-recurring-tooltip = Terugkerend ticket
kanban-recurring-aria = Terugkerend
kanban-sla-aria = SLA-status
calendar-today = Vandaag
calendar-anchor-label = Anker
calendar-anchor-tooltip = Het ankerveld wordt bepaald door de opgeslagen weergave; de kiezer komt in een latere update.
calendar-anchor-due-date = Vervaldatum
calendar-anchor-created = Aangemaakt
calendar-anchor-last-activity = Laatste activiteit
gantt-today = Vandaag
gantt-title = Gantt
user-cell-missing-tooltip = Deze gebruiker bestaat niet meer
user-cell-unknown = Onbekend
user-settings-managing-for = Instellingen beheren voor
user-settings-groups-title = Groepen
user-settings-role-management-title = Rolbeheer
user-settings-account-setup-title = Accountinstelling
user-settings-account-setup-pending = In afwachting
user-settings-invitation-pending = Uitnodiging in afwachting
user-settings-resend-invitation-title = Uitnodigingsmail opnieuw sturen
user-settings-danger-zone-title = Gevarenzone
user-settings-danger-zone-subtitle = Onomkeerbare en destructieve acties
user-settings-delete-modal-title = Accountverwijdering bevestigen
user-settings-delete-item-profile = Profielinformatie en instellingen
user-settings-delete-item-tickets = Alle tickets gemaakt door of toegewezen aan deze gebruiker
user-settings-delete-item-comments = Reacties en activiteitengeschiedenis
user-settings-delete-item-access = Toegang tot alle systemen en bronnen
user-settings-password-placeholder = Voer je wachtwoord in
admin-plugins-list-title = Plug-ins
admin-plugins-list-aria-filter = Plug-ins filteren
admin-plugins-list-search-placeholder = Plug-ins zoeken
admin-plugins-list-uninstall-title = Plug-in verwijderen
inbox-title = Postvak in
inbox-aria-filter = Notificaties filteren
inbox-aria-bulk-actions = Bulkacties
inbox-aria-clear-selection = Selectie wissen
inbox-aria-select-all = Alle notificaties selecteren
notifications-bell-aria-trigger = Notificaties
notifications-bell-aria-filter = Notificaties filteren
editor-mentions-hint-select = Selecteren
editor-mentions-hint-close = Sluiten
editor-mentions-helper-type = Typ
editor-mentions-helper-suffix = om iemand te vermelden

# R1 auth UX errors
auth-mfa-check-failed = Kon MFA-status niet controleren
auth-mfa-setup-failed = Kon MFA niet instellen
auth-mfa-setup-failed-retry = MFA-instelling mislukt. Probeer het opnieuw.
auth-mfa-code-invalid = Voer een geldige code van 6 cijfers in
auth-mfa-secret-missing = MFA-geheim ontbreekt. Start de installatie opnieuw.
auth-mfa-verify-failed = Ongeldige verificatiecode. Probeer het opnieuw.
auth-mfa-enable-failed = Kon MFA niet inschakelen
auth-mfa-disable-failed = Kon MFA niet uitschakelen
auth-mfa-backup-codes-failed = Kon back-upcodes niet opnieuw genereren
auth-passkey-load-failed = Kon passkeys niet laden
auth-passkey-not-supported-browser = Passkeys worden niet ondersteund in deze browser
auth-passkey-not-supported-device = Passkeys worden niet ondersteund op dit apparaat
auth-passkey-max-reached = Maximaal aantal passkeys bereikt (10)
auth-passkey-registered = Passkey "{ $name }" geregistreerd
auth-passkey-registration-not-allowed = Registratie is geannuleerd of niet toegestaan
auth-passkey-already-registered = Deze passkey is al geregistreerd
auth-passkey-registration-cancelled = Registratie is geannuleerd
auth-passkey-register-failed = Kon passkey niet registreren
auth-passkey-login-success = Aangemeld met passkey
auth-passkey-auth-not-allowed = Authenticatie is geannuleerd of niet toegestaan
auth-passkey-none-registered = Geen passkeys geregistreerd voor dit account
auth-passkey-auth-cancelled = Authenticatie is geannuleerd
auth-passkey-login-failed = Kon niet aanmelden met passkey
auth-passkey-name-required = Naam voor de passkey is verplicht
auth-passkey-name-too-long = Naam van de passkey mag maximaal 100 tekens bevatten
auth-passkey-renamed = Passkey hernoemd
auth-passkey-rename-failed = Kon passkey niet hernoemen
auth-passkey-delete-password-required = Wachtwoord vereist om een passkey te verwijderen
auth-passkey-deleted = Passkey verwijderd
auth-passkey-incorrect-password = Onjuist wachtwoord
auth-passkey-delete-failed = Kon passkey niet verwijderen
ticket-data-load-failed = Ticket kon niet worden geladen. Probeer het later opnieuw.
plugins-load-failed = Kon plug-ins niet laden
search-failed = Zoeken is mislukt. Probeer het opnieuw.
auth-autologin-prompt = Meld je aan met je inloggegevens.
auth-login-rate-limited = Te veel verzoeken. Wacht even.
auth-login-network-error = Netwerkfout. Controleer je verbinding.
auth-login-backup-codes-low = Aanmelding gelukt. Genereer je back-upcodes binnenkort opnieuw, je hebt er nog 2 of minder.
ticket-audio-play-failed = Audio kon niet worden afgespeeld
asset-modal-load-failed = Activa konden niet worden geladen. Probeer het opnieuw.
project-modal-load-failed = Projecten konden niet worden geladen. Probeer het later opnieuw.
user-profile-load-failed = Gebruikersinformatie kon niet worden geladen
# R2 filter facets + ticket columns
filter-facet-title = Titel
filter-facet-status = Status
filter-facet-priority = Prioriteit
filter-facet-assignee = Toegewezen aan
filter-facet-sla = SLA
filter-facet-cycle = Cyclus
filter-assignee-unassigned = Niet toegewezen
filter-assignee-loading = Laden…
filter-cycle-option = Cyclus #{ $id }
filter-summary-n-selected = { $count } geselecteerd
tickets-column-id = #
tickets-column-id-description = Ticketnummer
tickets-column-title = Titel
tickets-column-title-description = Ticketonderwerp
tickets-column-status = Status
tickets-column-status-description = Workflowstatus
tickets-column-priority = Prioriteit
tickets-column-priority-description = Prioriteit
tickets-column-assignee = Toegewezen aan
tickets-column-assignee-description = Wie het ticket bezit
tickets-column-requester = Aanvrager
tickets-column-requester-description = Wie het ticket meldde
tickets-column-category = Categorie
tickets-column-category-description = Categorie van het ticket
tickets-column-cycle = Cyclus
tickets-column-cycle-description = Cyclus-lidmaatschap
tickets-column-due-date = Vervaldatum
tickets-column-due-date-description = Kalender-deadline
tickets-column-last-activity = Bijgewerkt
tickets-column-last-activity-description = Wanneer het ticket voor het laatst veranderde
tickets-column-created-at = Aangemaakt
tickets-column-created-at-description = Wanneer het ticket werd geopend
tickets-column-sla = SLA
tickets-column-sla-description = SLA-pil (groen / amber / rood)
tickets-column-kb-gap = KB
tickets-column-kb-gap-description = Kennislacune-signaal
tickets-column-devices = Activa
tickets-column-devices-description = Aantal betrokken activa
tickets-column-recurrence = Herh.
tickets-column-recurrence-description = Markering voor terugkerend ticket

# Backend HTTP error responses (R3)
backend-error-auth-required = Authenticatie vereist.
backend-error-user-not-found = Gebruikersaccount niet gevonden.
backend-error-comment-fetch-failed = Reacties laden mislukt.
backend-error-comment-create-failed = Reactie maken mislukt.
backend-error-comment-not-found = Reactie niet gevonden.
backend-error-attachment-not-found = Bijlage niet gevonden.
backend-error-attachment-delete-failed = Bijlage verwijderen mislukt.

# S2 backend handler error sweep
backend-error-validation = Validatiefout.
backend-error-passkey-max-reached = Maximaal aantal passkeys bereikt.
backend-error-bad-request = Ongeldig verzoek.
backend-error-search-failed = Zoeken is mislukt.
backend-error-search-rebuild-failed = Index opnieuw opbouwen is mislukt.

# S1 frontend registries + defaults
builtin-view-my-open-name = Mijn openstaande
builtin-view-my-open-description = Aan jou toegewezen openstaande tickets
builtin-view-all-active-name = Alle actieve
builtin-view-all-active-description = Elk ticket dat niet is opgelost of geannuleerd
builtin-view-triage-name = Triage
builtin-view-triage-description = Tickets die nog ingedeeld moeten worden
builtin-view-calendar-name = Agenda
builtin-view-calendar-description = Tickets op hun vervaldatum geplaatst
workflow-category-triage = Triage
workflow-category-backlog = Wachtrij
workflow-category-active = Actief
workflow-category-in-review = In review
workflow-category-done = Klaar
workflow-category-cancelled = Geannuleerd
assignment-method-direct-user-name = Directe gebruiker
assignment-method-direct-user-description = Direct toewijzen aan een specifieke gebruiker
assignment-method-group-round-robin-name = Rondgang (groep)
assignment-method-group-round-robin-description = Verdeel de toewijzing gelijkmatig over de groepsleden
assignment-method-group-random-name = Willekeurig (groep)
assignment-method-group-random-description = Kies willekeurig een groepslid voor elk ticket
assignment-method-group-queue-name = Groepswachtrij
assignment-method-group-queue-description = Toewijzen aan groepswachtrij (gebruikers claimen tickets)
tickets-category-none = Geen categorie
tickets-menu-flag-for-docs = Markeren voor documentatie
tickets-menu-delete = Ticket verwijderen
docs-author-system = Systeem
docs-untitled-page = Zonder titel
profile-role-user-label = Gebruiker
profile-role-user-description = Kan tickets aanmaken en toegewezen middelen bekijken
profile-role-technician-label = Agent
profile-role-technician-description = Kan tickets en activa beheren en andere gebruikers helpen
profile-role-admin-label = Beheerder
profile-role-admin-description = Volledige toegang tot alle systeemfuncties en gebruikersbeheer
tickets-grouping-no-cycle = Geen cyclus
list-grouping-none = No grouping
list-grouping-trigger = Group by
views-column-picker-trigger = Columns
views-column-picker-reset = Reset columns
views-column-resize-handle-tooltip = Drag to resize
assets-list-grouping-warranty = Warranty
assets-list-grouping-kind = Type
assets-list-grouping-manufacturer = Manufacturer
assets-list-grouping-manufacturer-none = No manufacturer
assets-list-grouping-location = Location
assets-list-grouping-location-none = No location
assets-list-grouping-primary-user = Primary user
tickets-grouping-all = Alle

# T batch: final sweep
error-api-server = Er is een serverfout opgetreden. Probeer het later opnieuw.
error-api-validation = De opgegeven gegevens zijn ongeldig.
error-api-generic = Er is een fout opgetreden bij het verwerken van uw verzoek.
plugin-loader-error = Plugins laden is mislukt.
seed-welcome-page-title = Welkom bij Nosdesk
email-notice-security = Beveiligingsmelding
email-notice-security-critical = Kritieke beveiligingsmelding
email-notice-getting-started = Aan de slag
email-notice-success = Geslaagd
email-link-fallback-prompt = Of kopieer en plak deze link in uw browser:
email-footer-rights = Alle rechten voorbehouden.
email-footer-automated = Dit is een geautomatiseerd bericht. Reageer hier niet rechtstreeks op.
markdown-embed-depth-limit = [Limiet voor insluitdiepte bereikt]
markdown-embed-circular = [Circulaire insluiting gedetecteerd]
markdown-embed-reference = Ingesloten: { $title }
markdown-embed-reference-fallback = [Ingesloten: { $title }]

# V batch: editor plugins + SSE
editor-embed-empty-document = Leeg document
editor-embed-load-failed = Document laden is mislukt
editor-embed-open-document = Document openen
editor-loading = Laden…
sse-connection-failed = Verbinding mislukt.
sse-no-auth-token = Niet aangemeld.
auth-microsoft-logout-failed = Microsoft afmelden is mislukt.
editor-ticket-link-not-found = Ticket #{ $id } niet gevonden

# W batch: pluralization fixes
notifications-inbox-unread-count =
    { $count ->
        [one] { $count } ongelezen melding
       *[other] { $count } ongelezen meldingen
    }
gantt-tickets-in-view =
    { $count ->
        [one] { $count } ticket zichtbaar
       *[other] { $count } tickets zichtbaar
    }
bulk-bar-select-all-matching = Alle { $count } selecteren
bulk-bar-clear = Wissen
bulk-bar-selected-generic =
    { $count ->
        [one] { $count } geselecteerd
       *[other] { $count } geselecteerd
    }
bulk-bar-all-selected-generic = Alle { $count } geselecteerd
bulk-bar-tickets-selected =
    { $count ->
        [one] { $count } ticket geselecteerd
       *[other] { $count } tickets geselecteerd
    }
bulk-bar-tickets-all-selected = Alle { $count } tickets geselecteerd
bulk-bar-users-selected =
    { $count ->
        [one] { $count } gebruiker geselecteerd
       *[other] { $count } gebruikers geselecteerd
    }
bulk-bar-users-all-selected = Alle { $count } gebruikers geselecteerd
bulk-bar-devices-selected =
    { $count ->
        [one] { $count } apparaat geselecteerd
       *[other] { $count } apparaten geselecteerd
    }
bulk-bar-devices-all-selected = Alle { $count } activa geselecteerd
inbox-no-unread = U hebt geen ongelezen meldingen.
gantt-tickets-of-total-in-view =
    { $count ->
        [one] { $visible } van { $count } ticket zichtbaar
       *[other] { $visible } van { $count } tickets zichtbaar
    }
saved-view-name-this = Geef deze weergave een naam
saved-view-copy-suffix = Kopie van { $name }
