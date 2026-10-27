<div align="center">
  <img src="https://cdn.iasoft.dev/okoala/logo.png" alt="OKoala Logo" width="190" style="border-radius: 1.15rem;"/>
  <br/>

  <img src="https://img.shields.io/badge/Email-darkred.svg" alt=""/>
  <img src="https://img.shields.io/badge/Chat-darkgreen.svg" alt=""/>
  <img src="https://img.shields.io/badge/Calendar-darkblue.svg" alt=""/>

  <p style="padding-top: 0.2rem;">
    <b>OKoala: Open Source Collaboration Platform</b>
  </p>
</div>

## Overview

## License

Copyright © 2026 IASoft (PVT) LTD.

Licensed under the [AGPL, version 3.0](https://www.gnu.org/licenses/agpl-3.0.en.html). See [LICENSE](LICENSE).

## Use of AI in this project

OKoala is built by the IASoft engineering team, who utilizes Claude (by Anthropic) to speed up the development. 

The engineering team designs every component and writes the security core by hand: authorisation, tenant isolation and
secret handling (listed in [TRUST_LEVELS.toml](TRUST_LEVELS.toml)).

Claude writes most of the other code, and the team reviews every change before it is published here. Commits written by Claude can be isolated via `Co-Authored-By` trailer.
