; Harness for telefonica-clips/branches/65x/test_suite/factsav.clp
; Detected constructs: deftemplate: MAIN::A, MAIN::B, BAR::C, BAR::D, BAR::E, WOZ::G, WOZ::F; defmodule: MAIN, BAR, WOZ, FOO
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
