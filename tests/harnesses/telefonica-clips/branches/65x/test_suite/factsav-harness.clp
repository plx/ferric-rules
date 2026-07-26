; Harness for telefonica-clips/branches/65x/test_suite/factsav.clp
; Detected constructs: deftemplate: MAIN::A, MAIN::B, BAR::C, BAR::D, BAR::E, WOZ::G, WOZ::F; defmodule: MAIN, BAR, WOZ, FOO
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-965f2da10aff305dce6e2f7689ab259ab4a1947b204983d1b4f4a4d7861b6d93-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-965f2da10aff305dce6e2f7689ab259ab4a1947b204983d1b4f4a4d7861b6d93|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-965f2da10aff305dce6e2f7689ab259ab4a1947b204983d1b4f4a4d7861b6d93|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-965f2da10aff305dce6e2f7689ab259ab4a1947b204983d1b4f4a4d7861b6d93|COMPLETE" crlf))
