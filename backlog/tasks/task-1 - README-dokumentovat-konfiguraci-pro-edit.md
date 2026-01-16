---
id: TASK-1
title: README dokumentovat konfiguraci pro edit
status: Done
assignee:
  - '@myself'
created_date: '2026-01-16 06:21'
updated_date: '2026-01-16 11:07'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Dokumentovat v README.md podporu pro schvalování editací souborů (Write/Edit/MultiEdit tools).Aktuálně README zmíní jen Bash hooks, ale Dippy umí i editační pravidla (allow-edit, ask-edit, deny-edit) která jsou zmíněná jen v docs/config-v1.md jako 'Proposal'. Potřebuje přidat sekci do README s příkladem jak to zapnout a používat.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Přidat sekci 'File Edit Approval' do README.md
- [x] #2 Vysvětlit jak upravit matcher v settings.json (Bash|Write|Edit|MultiEdit)
- [x] #3 Uvést příklady allow-edit, ask-edit, deny-edit pravidel
<!-- AC:END -->
