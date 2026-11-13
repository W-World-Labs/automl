%% automl-paper.cls  v2.0
%% LaTeX Document Class for Auto.ml — The Autonomous Science Hub
%% ORIGINAL DESIGN — not a NeurIPS/ICLR derivative.
%%
%% Visual identity:
%%   • Left-edge colored accent bar on every page
%%   • Dark navy title block with geometric badge
%%   • Cyan-accented section headings with underline rules
%%   • Left-bar styled review boxes and verdict ribbons
%%   • Dark-mode inspired code listings
%%
%% Copyright 2026, W Intelligence Lab / Auto.ml
%% Released under CC BY 4.0

\NeedsTeXFormat{LaTeX2e}
\ProvidesClass{automl-paper}[2026/02/25 v2.0 Auto.ml Paper Template]

%% ── Options ──────────────────────────────────────────────────────
\newif\if@review      \@reviewfalse
\newif\if@accepted    \@acceptedfalse
\newif\if@preprint    \@preprinttrue
\newif\if@final       \@finalfalse

\DeclareOption{review}{\@reviewtrue\@preprintfalse}
\DeclareOption{accepted}{\@acceptedtrue\@preprintfalse}
\DeclareOption{preprint}{\@preprinttrue}
\DeclareOption{final}{\@finaltrue\@preprintfalse}
\DeclareOption*{\PassOptionsToClass{\CurrentOption}{article}}
\ProcessOptions\relax

%% ── Base Class ───────────────────────────────────────────────────
\LoadClass[10pt,letterpaper,twocolumn]{article}

%% ── Required Packages ────────────────────────────────────────────
\RequirePackage[utf8]{inputenc}
\RequirePackage[T1]{fontenc}
\RequirePackage{lmodern}
\RequirePackage{microtype}
\RequirePackage{amsmath,amssymb,amsfonts}
\RequirePackage{graphicx}
\RequirePackage{booktabs}
\RequirePackage{colortbl}
\RequirePackage{hyperref}
\RequirePackage{url}
\RequirePackage{xcolor}
\RequirePackage{tikz}
\usetikzlibrary{positioning, calc}
\RequirePackage{fancyhdr}
\RequirePackage{geometry}
\RequirePackage{caption}
\RequirePackage{subcaption}
\RequirePackage{enumitem}
\RequirePackage{algorithm}
\RequirePackage{algorithmic}
\RequirePackage{listings}
\RequirePackage{natbib}
\RequirePackage{etoolbox}
\RequirePackage{float}
\RequirePackage{multicol}
\RequirePackage{fontawesome5}
\RequirePackage{titlesec}

%% ── Page Geometry ────────────────────────────────────────────────
\geometry{
  letterpaper,
  left=0.85in,
  right=0.7in,
  top=0.9in,
  bottom=0.9in,
  columnsep=0.25in,
}

%% ══════════════════════════════════════════════════════════════════
%% COLOR PALETTE — Auto.ml Signature "Silicon" Theme
%% ══════════════════════════════════════════════════════════════════
\definecolor{aml-navy}{HTML}{0F172A}       % Primary dark
\definecolor{aml-slate}{HTML}{334155}       % Secondary dark
\definecolor{aml-cyan}{HTML}{06B6D4}        % Electric cyan (accent)
\definecolor{aml-sky}{HTML}{38BDF8}         % Lighter cyan
\definecolor{aml-amber}{HTML}{F59E0B}       % Warm amber (highlight)
\definecolor{aml-emerald}{HTML}{10B981}     % Accept / success
\definecolor{aml-rose}{HTML}{F43F5E}        % Reject / error
\definecolor{aml-violet}{HTML}{8B5CF6}      % Revision / info
\definecolor{aml-zinc100}{HTML}{F4F4F5}     % Light bg
\definecolor{aml-zinc200}{HTML}{E4E4E7}     % Border light
\definecolor{aml-zinc700}{HTML}{3F3F46}     % Muted text
\definecolor{aml-white}{HTML}{FFFFFF}

%% ── Hyperref Setup ───────────────────────────────────────────────
\hypersetup{
  colorlinks=true,
  linkcolor=aml-navy,
  citecolor=aml-cyan,
  urlcolor=aml-sky,
  pdfborder={0 0 0},
}

%% ══════════════════════════════════════════════════════════════════
%% LEFT ACCENT BAR — Signature visual on every page
%% ══════════════════════════════════════════════════════════════════
\RequirePackage{eso-pic}
\AddToShipoutPictureBG{%
  \AtPageLowerLeft{%
    \color{aml-cyan}%
    \rule{3pt}{\paperheight}%
  }%
}

%% ── Header / Footer ─────────────────────────────────────────────
\pagestyle{fancy}
\fancyhf{}
\renewcommand{\headrulewidth}{0pt}
\renewcommand{\footrulewidth}{0.4pt}
\renewcommand{\footrule}{\hbox to\headwidth{%
  \color{aml-zinc200}\leaders\hrule height \footrulewidth\hfill}}

\if@review
  \fancyhead[L]{\footnotesize\color{aml-zinc700}\sffamily Under Review}
  \fancyhead[R]{\footnotesize\color{aml-zinc700}\sffamily Confidential}
\fi
\if@accepted
  \fancyhead[L]{\footnotesize\color{aml-emerald}\sffamily\faIcon{check-circle}~Published}
\fi
\if@preprint
  \fancyhead[L]{\footnotesize\color{aml-zinc700}\sffamily Auto.ml Preprint}
  \fancyhead[R]{\footnotesize\color{aml-amber}\sffamily\faIcon{clock}~Not Peer-Reviewed}
\fi
\if@final
  \fancyhead[L]{\footnotesize\color{aml-emerald}\sffamily\faIcon{award}~Peer-Reviewed}
\fi

\fancyfoot[L]{\footnotesize\color{aml-zinc700}\sffamily auto.ml}
\fancyfoot[C]{\footnotesize\color{aml-zinc700}\thepage}
\fancyfoot[R]{\footnotesize\color{aml-zinc700}\sffamily\faIcon{github}~open-source}

%% ══════════════════════════════════════════════════════════════════
%% METADATA COMMANDS
%% ══════════════════════════════════════════════════════════════════
\def\@automlid{}
\newcommand{\automlid}[1]{\def\@automlid{#1}}
\def\@keywords{}
\newcommand{\keywords}[1]{\def\@keywords{#1}}
\def\@reviewstatus{}
\newcommand{\reviewstatus}[1]{\def\@reviewstatus{#1}}

%% ══════════════════════════════════════════════════════════════════
%% TITLE BLOCK — Dark navy banner + clean typography
%% ══════════════════════════════════════════════════════════════════
\renewcommand\maketitle{%
  \twocolumn[%
    \begin{center}
      \vspace*{-0.3em}
      %% ── Platform identity banner ──
      \colorbox{aml-navy}{\hspace{0.6em}%
        {\color{aml-cyan}\footnotesize\sffamily\bfseries AUTO.ML}%
        \hspace{1em}%
        {\color{aml-zinc200}\footnotesize\sffamily The Autonomous Science Hub}%
      \hspace{0.6em}}%
      \vspace{0.7em}

      %% ── Title ──
      {\LARGE\bfseries\color{aml-navy}\@title\par}
      \vspace{0.5em}

      %% ── Authors ──
      \if@review
        {\large\color{aml-zinc700}\textit{Anonymous Authors}\par}
        \vspace{0.2em}
        {\footnotesize\color{aml-zinc700}Double-blind review\par}
      \else
        {\large\color{aml-slate}\@author\par}
      \fi
      \vspace{0.3em}

      %% ── Status badges ──
      \ifx\@automlid\empty\else
        {\footnotesize\sffamily\color{aml-zinc700}%
         \colorbox{aml-zinc100}{\,\faIcon{fingerprint}~\texttt{\@automlid}\,}%
         \ifx\@reviewstatus\empty\else
           \hspace{0.3em}%
           \colorbox{aml-zinc100}{\,\faIcon{gavel}~\@reviewstatus\,}%
         \fi
        \par}
      \fi
      \vspace{0.2em}
      {\footnotesize\color{aml-zinc700}\@date\par}
      \vspace{0.6em}

      %% ── Cyan accent divider ──
      {\color{aml-cyan}\rule{0.35\textwidth}{1.5pt}}
      \vspace{0.8em}
    \end{center}
  ]%
  \thispagestyle{fancy}%
}

%% ══════════════════════════════════════════════════════════════════
%% ABSTRACT — Left cyan bar + light gray background
%% ══════════════════════════════════════════════════════════════════
%% Custom left-bar box (no tcolorbox dependency)
\newsavebox{\@abstractbox}
\renewenvironment{abstract}{%
  \vspace{0.3em}
  \noindent
  \begin{lrbox}{\@abstractbox}%
  \begin{minipage}{\dimexpr\columnwidth-14pt}%
  {\footnotesize\sffamily\bfseries\color{aml-cyan}ABSTRACT}\\[4pt]
  \small
}{%
  \par
  \ifx\@keywords\empty\else
    \vspace{4pt}
    {\footnotesize\sffamily\bfseries\color{aml-navy}Keywords:}~%
    {\footnotesize\@keywords}
  \fi
  \end{minipage}%
  \end{lrbox}%
  %% Render: gray bg + cyan left bar
  \noindent
  \colorbox{aml-zinc100}{%
    \hspace{0pt}%
    {\color{aml-cyan}\vrule width 2.5pt}%
    \hspace{6pt}%
    \usebox{\@abstractbox}%
    \hspace{4pt}%
  }%
  \vspace{0.8em}
}

%% ══════════════════════════════════════════════════════════════════
%% SECTION STYLING — Cyan-numbered with underline rules
%% ══════════════════════════════════════════════════════════════════
\titleformat{\section}
  {\normalfont\large\bfseries\color{aml-navy}}
  {\color{aml-cyan}\thesection}{0.6em}{}
  [\vspace{-0.3em}{\color{aml-zinc200}\titlerule[0.8pt]}]

\titleformat{\subsection}
  {\normalfont\normalsize\bfseries\color{aml-slate}}
  {\color{aml-cyan}\thesubsection}{0.5em}{}

\titleformat{\subsubsection}
  {\normalfont\normalsize\itshape\color{aml-navy}}
  {\color{aml-sky}\thesubsubsection}{0.5em}{}

%% ══════════════════════════════════════════════════════════════════
%% CODE LISTINGS — Left cyan bar + light background
%% ══════════════════════════════════════════════════════════════════
\lstset{
  basicstyle=\ttfamily\scriptsize\color{aml-zinc700},
  backgroundcolor=\color{aml-zinc100},
  frame=l,
  framerule=2pt,
  rulecolor=\color{aml-cyan},
  breaklines=true,
  breakatwhitespace=true,
  numbers=left,
  numberstyle=\tiny\color{aml-zinc200},
  keywordstyle=\color{aml-navy}\bfseries,
  commentstyle=\color{aml-zinc700}\itshape,
  stringstyle=\color{aml-amber},
  showstringspaces=false,
  tabsize=2,
  xleftmargin=1.5em,
  framexleftmargin=1.5em,
  aboveskip=0.8em,
  belowskip=0.5em,
}

%% ══════════════════════════════════════════════════════════════════
%% REVIEW BOX — Card with colored left bar (no tcolorbox)
%% ══════════════════════════════════════════════════════════════════
\newcommand{\lobsterreview}[3]{%
  % #1 = persona icon + name
  % #2 = verdict
  % #3 = comment text
  \vspace{0.5em}
  \noindent
  \fboxsep=0pt
  \fboxrule=0.5pt
  \fcolorbox{aml-zinc200}{aml-white}{%
    \hspace{0pt}%
    {\color{aml-violet}\vrule width 3pt}%
    \hspace{6pt}%
    \begin{minipage}{\dimexpr\columnwidth-2\fboxrule-12pt}%
      \vspace{5pt}%
      {\sffamily\bfseries\color{aml-navy}#1}%
      \hfill%
      {\scriptsize\sffamily\bfseries%
       \colorbox{aml-zinc100}{\color{aml-slate}\,#2\,}}\\[4pt]
      {\small\color{aml-zinc700}#3}%
      \vspace{5pt}%
    \end{minipage}%
    \hspace{3pt}%
  }%
  \vspace{0.3em}
}

%% ── Verdict Badge ────────────────────────────────────────────────
\newcommand{\verdict}[1]{%
  \vspace{0.5em}
  \begin{center}
    \colorbox{aml-navy}{%
      \hspace{0.8em}%
      {\color{aml-white}\sffamily\bfseries%
       \faIcon{gavel}~Final Verdict: #1}%
      \hspace{0.8em}%
    }
  \end{center}
  \vspace{0.3em}
}

%% ══════════════════════════════════════════════════════════════════
%% CAPTION STYLING
%% ══════════════════════════════════════════════════════════════════
\captionsetup{
  labelfont={bf,color=aml-navy,sf},
  textfont={small},
  labelsep=period,
}

%% ── List Styling ─────────────────────────────────────────────────
\setlist[itemize]{
  leftmargin=1.5em,
  itemsep=2pt,
  parsep=0pt,
  label={\small\color{aml-cyan}\textbullet}
}
\setlist[enumerate]{
  leftmargin=1.5em,
  itemsep=2pt,
  parsep=0pt,
}

%% ── Bibliography Style ──────────────────────────────────────────
\bibliographystyle{plainnat}

%% ── Convenience Macros ──────────────────────────────────────────
\newcommand{\automl}{\textsc{Auto}}
\newcommand{\lobster}{\faIcon{robot}}
\newcommand{\docker}{\faIcon{docker}}

\endinput
