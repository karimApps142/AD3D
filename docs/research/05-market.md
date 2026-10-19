# LuBan market and competitive analysis (as of 2026-10-19)

## Key findings
- **LuBan is a paid, active product.** Prices from https://www.luban3d.com/license: 1 month $30, 1 year $150, permanent $750, USB dongle $1,200. Payments on a 1-month or 1-year licence count toward the permanent price.
  - Without a licence it runs and previews, but you can only save files up to 1 MB.
  - Free 30-day full trial, requested through a web form with the machine's License ID. There is no licence key. Timed licences need internet; permanent and USB licences don't.
  - The page says USB licences work on Windows 7/10 and Mac OS 10/11; Linux is not supported. A current Mac build is UNCONFIRMED.
  - It is still active. The "LuBan Squama" video went up on 2026-09-30 (716 views when checked). The @luban9604 YouTube channel has about 7.28K subscribers and 171 videos.
  - Commercial-use terms are not stated (UNCONFIRMED).
- **The broad 3D-model-to-flat-parts tools from big vendors are dead or frozen.** 123D Make was discontinued (Nov 2016 to Mar 2017). Slicer for Fusion 360 has been deprecated since Aug 2020. Meshmixer has not been developed since 2021. No maintained replacement does all of these together:
  - several kinds of cut model (stacked slices, interlocking waffle slices, curved, radial, folded panels)
  - splitting and laying parts out to fit a given machine bed
  - an assembly guide

  Autodesk forum users still say "I haven't found a good alternative yet."
- **What exists now is single-purpose web tools, or tools tied to one hardware brand.**
  - Lithophanes: ItsLitho, LithophaneMaker, 3dp.rocks, Bambu MakerLab.
  - Stacked slices: Atomm (xTool's site), Kiri:Moto, BurntStuff.
  - Unfolding for papercraft: Pepakura, Blender's Export Paper Model.
  - Everything at once only in Rhino plus Grasshopper plugins, which is for experts.

## Comparison table

Overlap means overlap with LuBan's features.

| Product | Price | Platform | Overlap with LuBan | Status / activity |
|---|---|---|---|---|
| **LuBan (luban3d.com)** | $30/mo, $150/yr, $750 permanent, $1,200 USB; free use limited to 1 MB saves | Windows; Mac for USB licence per licence page (UNCONFIRMED) | All (baseline): lithophane, stack, waffle, plate, relief, module, wireframe, eyeglasses, Photo Magic, Squama, splitting to fit the bed | Active (video 2026-09-30) |
| **Autodesk 123D Make** | Was free | Windows, Mac, web, iOS | Stacked, interlocked, curve, radial, folded panels; page-size layout | Discontinued Nov 2016 to Mar 2017; Autodesk said it was folded into Slicer / Fusion 360 |
| **Slicer for Fusion 360** | Free | Windows, Mac (runs on its own) | Stacked, interlocked, curve, radial, folded panels, 3D slices; sheet layout; assembly steps | Deprecated Aug 2020, no longer maintained. Final build still hosted by Autodesk. Whether it is back in the App Store is UNCONFIRMED (forum reports conflict) |
| **Autodesk Fusion (built-in tools)** | Free for personal use (non-commercial, under $1k/yr revenue, limited features). Paid about $680/yr (third-party figure, UNCONFIRMED) | Windows, Mac | Manual only: Split Body for slices plus Arrange for layout. Arrange is not in the free Personal tier | Active, but there is no one-click slicer |
| **Meshmixer** | Free | Windows, Mac (no Apple Silicon build) | Mesh processing, hollowing, patterns | Development and support ended 2021; last version 3.5; still downloadable |
| **Pepakura Designer 6** | $69 one-time (50% academic discount) | Windows only; v6.3.8 | Unfolding into flat paper or card patterns (close to "plate" / folded panels) | Active (v6.2 added a body-fit reference avatar) |
| **Blender "Export Paper Model"** | Free (GPL-3) | Windows, Mac, Linux (Blender add-on) | Unfold to SVG | Active: v1.4.2, Oct 2025, for Blender 5.0 |
| **ItsLitho** | Free (paid tiers UNCONFIRMED; it has a store) | Web | Lithophanes: flat, arc, cylinder, sphere, vase, lamp, bell, pumpkin, frames, colour lithophanes; light preview | Active (© 2026) |
| **LithophaneMaker.com** | Free web tool. A related desktop app is listed at $22 one-time or $13/mo; who makes it is UNCONFIRMED | Web; desktop app is Windows | Lithophanes (many shapes) | Live (page renders with JavaScript); maintenance level UNCONFIRMED |
| **3dp.rocks lithophane (Nested Cube)** | Free | Web | Basic image-to-lithophane height map | Old tool; current maintenance UNCONFIRMED (fetch returned an empty page) |
| **Bambu MakerWorld MakerLab** | Free with Bambu account (terms UNCONFIRMED) | Web, exports 3MF | Mono and colour (CMYK) lithophanes, Relief Sculpture Maker | Active; built for Bambu printers |
| **Atomm (xTool) 3D Model Layer Slicer** | Free or credits, UNCONFIRMED | Web | Stacked slices only; board thickness, slice direction, alignment holes. No waffle or bed nesting mentioned | Active; xTool's own site |
| **BurntStuff STL Relief Slicer** | Free (tip jar) | Web, runs locally in the browser | Relief and fin slices, freestanding cross-sections, slotted rails, sheet-size check, numbered parts, SVG/DXF, multi-sheet zip | New: announced 2026-08-15 by one developer |
| **Kiri:Moto (Grid.Space)** | Free, MIT licence | Web, runs locally in the browser | Laser cutting and stacking slices; also 3D printing and CNC | Active open source |
| **Plycutter** | Free, open source | Command line (Python) | STL to finger-jointed DXF, for models built from sheets | Niche; activity UNCONFIRMED |
| **Rhino + Grasshopper plugins (Waffle, Ivy, OpenNest, Kangaroo, Text Tag 3D)** | Plugins free; Rhino about $995 (UNCONFIRMED) | Windows, Mac (Waffle lists Rhino 6/7 on Windows) | Everything: waffle, unfolding, nesting, part labels, panelling | Active, but for experts and you assemble the pipeline yourself |
| **Glowforge app + Premium** | Premium $50/mo or $239/yr (support FAQ; also lists $600/yr) | Web | 2D design, AI "Magic Canvas"; no slicing of 3D models | Company restructured in 2025 through an assignment for the benefit of creditors (an alternative to bankruptcy); co-founders bought the assets, about 20 staff |
| **Cricut Design Space / Access** | Access Standard $9.99/mo or $95.88/yr; Premium about $14.99/mo (UNCONFIRMED) | Windows, Mac, iOS, Android | 2D cutting only; little overlap | Active, tied to Cricut machines |
| **xTool Studio (formerly XCS)** | Free for xTool owners | Windows, Mac, web | 2D laser work; manual layering; native 3D slicing UNCONFIRMED (no evidence found) | Active |
| **LightBurn** | Paid (current price UNCONFIRMED) | Windows, Mac, Linux | "3D Sliced" mode works from greyscale depth maps, galvo lasers only. STL slicing "is coming" (secondhand forum report, 2026-06) | Active. Biggest threat if it ships STL slicing |
| **Online image-to-STL tools (imageonline.io, JLC3DP, Grayscale-Image-to-STL on GitHub)** | Free | Web, often runs in the browser | Height-map relief and lithophane; overlaps Photo Magic and relief | Many, mostly interchangeable |
| **Snapmaker Luban** | Free, AGPL-3 | Windows, Mac, Linux | 3-in-1 control software for 3D printing, laser and CNC on Snapmaker machines; laser and CNC toolpaths. **Name clash** | Active; Snapmaker Orca (a fork of OrcaSlicer) announced May 2025 as its successor. Luban is not discontinued |

## Market size signals
- **3D printers:** CONTEXT says shipments of entry-level 3D printers (under $2,500) passed 1 million units in Q1 2025 (+15% year on year). They grew 26% for full-year 2025 and 47% in Q4 2025. Bambu Lab has a 37% share, and Chinese vendors over 90%. The full-year unit count is UNCONFIRMED.
- **Desktop lasers:**
  - Paid market reports put the 2025 market at about $1.8B, forecast to reach $3.4B to $4.5B by 2034 (7.8% to 10.5% a year). These are estimates.
  - xTool's Hong Kong IPO filing claims 47% of the market by sales value (first nine months of 2025), six times the next player.
  - xTool's revenue is reported as CNY 2.476B, but sources disagree on whether that is full-year 2024 or only its first nine months. Revenue for the first nine months of 2025 was CNY 1.777B.
- **Read-across:** Millions of new printer and laser owners a year, with companies building free design tools into their ecosystems (MakerLab, Atomm, Glowforge Magic Canvas). Hardware makers use software to win and keep users. That pushes standalone tools toward free or cheap pricing, not $150/yr.

## Trademark risk with "LuBan": high, do not use the name
- Snapmaker holds US Reg. No. 6,494,587 for LUBAN (standard characters, Class 7), registered 2021-09-21 according to Justia. It covers machines, including 3D printers and laser engraving machines.
- Snapmaker also filed serials 90035519 and 90035546 on the same day. Their goods include "downloadable computer operating programs". Whether they registered is UNCONFIRMED: the USPTO's status system now needs an API key, and Justia blocked automated access.
- Snapmaker Luban is well-known free software in the same market (3D printing, laser, CNC), so the risk of confusion is high.
- Another LUBAN registration, No. 4,986,853 (2016), belongs to Yueqing Superfine Electric; its goods are UNCONFIRMED. Alibaba also uses "Luban" for an AI design product.
- EU, China and WIPO filings were not checked (UNCONFIRMED).
- **Recommendation:** pick a different, distinctive name and run a clearance search in Classes 9 and 42 before launch.

## The gap a new product could own
No maintained tool that works with any brand of machine and runs on any platform does all of the following:
1. Turns a 3D model or photo into several kinds of fabricable part: stacked, waffle, folded, lithophane, relief, wireframe.
2. Splits and nests parts automatically to a chosen machine bed, with part labels, an assembly guide and slot fit that allows for kerf.
3. Exports to all three processes: STL/3MF for printing, SVG/DXF for laser, and DXF or toolpaths for CNC.

LuBan comes closest, but it is a Windows desktop app with expensive licensing. The Autodesk tools that defined this category are gone. The web tools each do one thing, and the vendor tools lock you to their hardware.

## Positioning recommendation
- **Product:** a browser app (WebAssembly and WebGPU, processing on the user's device for privacy and low server cost). It would be the maintained successor to 123D Make / Fusion Slicer, plus a lithophane and relief studio.
  - Lead with what is unserved: waffle and stacked slicing with automatic splitting to the bed and nesting, and assembly instructions.
  - Lithophane and relief are table stakes (many free options), so include them but don't lead with them.
  - Add LuBan-style generative extras later: wireframe, module, Squama-like scale or tile patterns, eyeglasses.
- **Model:** free tier with full preview and exports up to a size or part-count limit. Pro at about $5 to $10/mo or about $49 to $79 one-time, well under LuBan's $150/yr and in line with Pepakura's $69. Optionally sell credits for cloud features such as image-to-3D AI.
  - Commercial-use licence in Pro (small shops sell laser and lithophane products).
  - Education and makerspace site licences. Former 123D and Slicer users were heavily in schools and makerspaces.
- **Distribution:** machine profiles for xTool, Glowforge, Bambu, Snapmaker, LightBurn-compatible lasers and Shapeoko-class CNCs, plus one-click "open in LightBurn / xTool Studio / Bambu Studio". Show up in Autodesk forum threads asking for a "Slicer replacement" and in maker YouTube.
- **Risks:**
  - Hardware vendors' free web tools (Atomm, MakerLab) widen their scope.
  - LightBurn adds STL slicing.
  - Free open-source tools (Kiri:Moto, BurntStuff) cap what people will pay.
  - The defences: breadth of techniques, bed-fit and assembly quality, and working with every brand.

## Sources
- LuBan: https://www.luban3d.com/ · https://www.luban3d.com/license · https://www.youtube.com/watch?v=Wd26qsSIRY8 · https://www.youtube.com/@luban9604
- Autodesk Slicer / 123D: https://autodesk.com/support/technical/article/caas/tsarticles/ts/3yg7zznS94MHNDG7KMV8Qg.html · https://forums.autodesk.com/t5/fusion-support-forum/fusion-360-slicer-alternative/td-p/10430957 · https://forums.autodesk.com/t5/fusion-design-validate-document/replacement-for-slicer-app/m-p/9921385/highlight/true · https://en.wikipedia.org/wiki/Autodesk_123D · https://www.autodesk.com/solutions/123d-apps · https://www.cgchannel.com/2016/12/autodesk-cans-its-123d-apps/ · https://www.makerspace.uq.edu.au/files/1863/TU-031-A%20Using%20Slicer%20for%20Fusion%20360%20for%20model%20production.pdf
- Fusion: https://autodesk.com/products/fusion-360/personal · https://vagon.io/blog/fusion-360-pricing
- Meshmixer: https://www.cgchannel.com/2021/12/5-industry-stories-you-may-have-missed-in-december-2021/ · https://forums.autodesk.com/t5/meshmixer-forum/meshmixers-future/m-p/10193042
- Pepakura: https://pepakura.tamasoft.co.jp/pepakura_designer/get-your-license-key/ · https://pepakura.tamasoft.co.jp/pepakura_designer/download/
- Blender: https://extensions.blender.org/add-ons/export-paper-model/versions/
- Lithophane tools: https://itslitho.com/ · https://lithophanemaker.com/ · https://alternativeto.net/software/lithophane-maker-desktop/about · https://3dp.rocks/lithophane/ · https://3dprint.com/37778/3d-printed-lithophane/ · https://all3dp.com/2/bambu-lab-makerworld-overview/ · https://forum.bambulab.com/t/am-i-allowed-to-use-the-lithophane-generator-to-sell-lithophanes/130835
- Laser slicing web tools: https://www.atomm.com/creativetools/3d-model-layer-slicer · https://www.burntstuff.com/tools/slicer/ · https://forum.lightburnsoftware.com/t/stl-relief-slicer/191839 · https://grid.space/ · https://github.com/GridSpace/grid-apps · https://tjltjl.github.io/plycutter/
- Rhino/Grasshopper: https://www.food4rhino.com/en/app/waffle · https://discourse.mcneel.com/t/unfolded-shape-with-flaps-ivy-in-opennest/151835 · https://interactivetextbooks.tudelft.nl/rhino-grasshopper/Grasshopper_Rhino_course/2_Knowledge_base/Digital_fabrication/Laser_cutting/!index.html
- LightBurn: https://docs.lightburnsoftware.com/galvo/3DSlice.html · https://forum.lightburnsoftware.com/t/stl-files-instead-of-depthmaps/190412
- Glowforge / Cricut / xTool: https://www.geekwire.com/2025/glowforge-hits-restart-after-restructuring-co-founders-acquire-key-assets-of-laser-engraver-startup/ · https://support.glowforge.com/hc/en-us/articles/4605387818267-Glowforge-Premium-Account-FAQ · https://glowforge.com/latest-improvements/premium-for-everyone-and-smartfit-improvements · https://cricut.com/en_us/cricut-access.html · https://help.cricut.com/hc/articles/360009503713-How-do-I-purchase-Cricut-Access · https://www.filehorse.com/download/file/BeV7Olk5iFoG_wNgiJ-2gnI2_l-If42wdPZY8j_86CPQ1bic4pTiqU1tvFatpnHDMM7yL_KgenKLGK54QUtwdN-bFDYfhgoO3_mFFs3Cpkg/
- Image-to-STL tools: https://jlc3dp.com/blog/image-to-stl · https://imageonline.io/image-to-stl/ · https://github.com/sysmaya/Grayscale-Image-to-STL
- Market: https://www.contextworld.com/quarterly-industrial-3d-printer-shipments-rebound-amid-continued-record-breaking-entry-level-growth · https://3dprintingindustry.com/news/context-report-entry-level-3d-printer-shipments-surge-as-industrial-segment-declines-241811/ · https://tomshardware.com/3d-printing/bambu-lab-overtakes-creality-as-the-worlds-top-selling-budget-3d-printer-brand · https://marketintelo.com/report/desktop-laser-engraver-market · https://dataintelo.com/report/desktop-laser-engraver-market · https://en.wikipedia.org/wiki/XTool · https://eu.36kr.com/en/p/3637407225070601 · https://www1.hkexnews.hk/app/sehk/2026/108021/a128793/sehk26010100112.pdf · https://makers101.com/xtool-hong-kong-ipo-filing-analysis/
- Trademark / Snapmaker: https://trademarks.justia.com/900/35/luban-90035504.html · https://trademarks.justia.com/owners/shenzhen-snapmaker-technologies-co-ltd-3640057 · https://uspto.report/TM/86771518 · https://github.com/Snapmaker/Luban · https://forum.snapmaker.com/t/snapmaker-luban-is-open-source-now-on-github/5601 · https://www.snapmaker.com/blog/open-source-philosophy-and-the-snapmaker-orca-project/