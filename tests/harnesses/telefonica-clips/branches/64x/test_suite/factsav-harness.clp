; Harness for telefonica-clips/branches/64x/test_suite/factsav.clp
; Detected constructs: deftemplate: MAIN::A, MAIN::B, BAR::C, BAR::D, BAR::E, WOZ::G, WOZ::F; defmodule: MAIN, BAR, WOZ, FOO
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1faaaf7a4c50b041024fcf93462e32e430cb1df9449017bc6ea1782eed8ac10e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1faaaf7a4c50b041024fcf93462e32e430cb1df9449017bc6ea1782eed8ac10e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1faaaf7a4c50b041024fcf93462e32e430cb1df9449017bc6ea1782eed8ac10e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1faaaf7a4c50b041024fcf93462e32e430cb1df9449017bc6ea1782eed8ac10e|COMPLETE" crlf))
