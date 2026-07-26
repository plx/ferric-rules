; Harness for telefonica-clips/branches/63x/test_suite/factsav.clp
; Detected constructs: deftemplate: MAIN::A, MAIN::B, BAR::C, BAR::D, BAR::E, WOZ::G, WOZ::F; defmodule: MAIN, BAR, WOZ, FOO
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7d376a7527f1d28d3d301548d68200c118d9425acabaccb85500d1ad128724f6-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7d376a7527f1d28d3d301548d68200c118d9425acabaccb85500d1ad128724f6|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7d376a7527f1d28d3d301548d68200c118d9425acabaccb85500d1ad128724f6|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7d376a7527f1d28d3d301548d68200c118d9425acabaccb85500d1ad128724f6|COMPLETE" crlf))
