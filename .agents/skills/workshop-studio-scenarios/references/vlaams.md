# Taal van de Workshop Studio: gesproken Vlaams

Alles wat het publiek of de presentator in de Studio ziet, staat in het Nederlands zoals het in Vlaanderen op een
werkvloer gesproken wordt: to the point, los maar correct. Dit bestand is de enige bron voor toon en woordkeuze.
Documentatie, code, commentaar, testnamen, ids en `tech`-regels blijven Engels.

## Toon

- Korte, actieve zinnen. Zeg wat er gebeurt en wie er last van heeft. Geen omhaal, geen marketingtaal.
- Spreek aan met "je" (nooit "u", nooit "ge/gij"). Meestal is aanspreken niet nodig: beschrijf wat er gebeurt.
- Gesproken, niet plat: geen dialect, geen tussenwerpsels. Schrijf zoals een collega het aan een andere afdeling
  uitlegt.
- Vlaamse woordkeuze waar die natuurlijk is: nakijken, opkuisen, mislopen ("wat loopt er mis"), verwittigen,
  historiek, op voorhand, in orde, een fout zit in, iets loopt vast.
- Geen Hollandismen: niet "nou", "hartstikke", "even" als stopwoord, "hoor", "leuk", "klopt als een bus", "gaaf".
- Geen letterlijk vertaald Engels: niet "het maakt zin", "op het einde van de dag", "dit is waar ... inkomt".
- Getallen: duizendtallen met een punt (3.432), decimalen met een komma (92,8). Datums: 21 november, 18:06.
- Omgevingen heten development, validatie en productie (nooit dev, val, prd in zichtbare tekst).

## Vakjargon blijft Engels

Geen geforceerde vertalingen. Deze woorden blijven zoals het team ze zegt, ook als werkwoord:

deploy / deployen / gedeployd, release / releasen, build / builden / gebuild, load (de load), run / runnen,
feed, check, test, retry, rollback, pipeline, branch, tag, commit, merge, vault, toolbox, package, selector, log,
sandbox, storage, fingerprint, dashboard, flow, demo, live, update, developer, operator, database.

Gewone Nederlandse woorden blijven gewoon Nederlands: tabel, rij, kolom, rapport, wachtwoord, versie, omgeving,
wijziging, fout, mail, bestand, instelling, stap, nacht.

## Vaste woorden (één woord per ding)

| Engels in de oude teksten | Vlaams in de Studio |
|---|---|
| feed | feed |
| table / report / management report | tabel / rapport / managementrapport |
| check / test | check / test |
| nightly load | nachtelijke load ("de nacht" als het kort moet) |
| rehearsal (de automatische controle van een voorstel) | testrun |
| practice run (een run zonder data) | dry run |
| proposal (een wijziging die nog niet goedgekeurd is) | voorstel |
| accept / accepted | goedkeuren / goedgekeurd |
| version / release | versie / release |
| package (de verzegelde versie met haar tools) | package |
| toolbox | toolbox |
| robot / robot account | robot / robotaccount |
| the shop | de webshop |
| safeguard | vangnet |
| harm | schade |
| clean-up / leftovers | opkuis / restanten |
| put live / go live | deployen / live zetten |
| go back one version | rollback naar de vorige versie |
| catalogue | catalogus |
| storage | storage |
| fingerprint | fingerprint |
| vault | vault |
| setting | instelling |
| hand-made change | manuele wijziging |
| loading history | laadhistoriek |
| logbook | logboek |
| maintenance step | opkuisstap |
| announcement (melding per ververste tabel) | melding |
| notice (de mail aan wie het rapport leest) | verwittiging |
| documentation / front page | documentatie / startpagina |
| environment | omgeving |
| development / validation / production | development / validatie / productie |
| scenario / flow / stage / step | scenario / flow / stap / stap |
| In place today / Next step | Vandaag al in gebruik / Volgende stap |
| Without it: | Zonder: |
| What goes wrong / The idea / The code / Live demo | Wat loopt er mis / Het idee / De code / Live demo |
| Getting ready / Prepare this demo | Klaarzetten / Demo klaarzetten |
| Technical details | Technische details |
| Recorded replay / Live | Opname / Live |
| rows / columns | rijen / kolommen |
| built / not run / failed / left out on purpose | gebuild / niet gerund / mislukt / bewust overgeslagen |
| refreshed | ververst |
| the dbt engine | engine (nooit "motor") |
| the select guard (refuses a build without a selection) | guard |
| safeguard in general, a rule that stops a mistake | vangnet (nooit "bewaker", "bewaking" of "beveiliging") |
| to build / built | builden / gebuild (nooit "zorgen voor de build") |
| data checks | datachecks (enkel "checks" als de context duidelijk is) |
| failure mail | foutmail (nooit "faalmail") |
| the practice system / practice data | oefensysteem / oefendata (nooit "demosysteem") |
| the database of a testrun | testdatabase |
| a private copy of the tables / of the code | privékopie / kopie (de kopie voor de release = de release branch) |
| a developer's own tables | eigen kopieën |
| the carrier / customer service | pakjesdienst / klantendienst |
| house rules | huisregels |
| the note next to a copy (`deployed.json`) | notitie |
| the day the feed changes format | omschakeldag |
| a heavy report that only runs on request | zwaar rapport |
| developer 1 / developer 2 | developer één / developer twee |
| a line on a card, a line of a code block | regel (een rij is een rij van een tabel of van het logboek) |
| depends on / built on | hangt af van / steunt op (niet "gebouwd op") |
| to report a late feed | een late feed melden (niet "noemen") |

## Glossarium: de sleutels

De sleutels van `_glossary.yaml` (en dus elke `[[term]]` in een tekst) heten:

catalogus, check, database, omgeving, feed, fingerprint, manuele wijziging, log, model, nachtelijke load, operator,
voorstel, testrun, release, robotaccount, sandbox, instelling, storage, tabel, test, vault, versie, dry run,
laadhistoriek, logboek, opkuisstap, melding, verwittiging, robot.

## Wat niet vertaald wordt

- Alles onder "Technische details" (ruwe logregels, commando's, namen van accounts en databases).
- `tech`-regels, ids van scenario's en stappen, bestandsnamen, namen van tabellen, macro's, jobs en variabelen.
- Tekst die uit de stack zelf komt en letterlijk getoond wordt (een regel uit een log, de tekst van dbt of Airflow die in een mail geciteerd wordt). De mails van de stack zelf (foutmail, verwittiging) zijn wel in dit Vlaams geschreven.
- De `line:`, `from:` en `to:` van een codeblok: die moeten letterlijk in de code staan.
