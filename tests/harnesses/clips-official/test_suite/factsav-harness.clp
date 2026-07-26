; Harness for clips-official/test_suite/factsav.clp
; Detected constructs: deftemplate: MAIN::A, MAIN::B, BAR::C, BAR::D, BAR::E, WOZ::G, WOZ::F; defmodule: MAIN, BAR, WOZ, FOO
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-90b26af068cbac8a94b0fe9cdfd7bc53e438ffb2cd1188d4a35627c2bdfd578d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-90b26af068cbac8a94b0fe9cdfd7bc53e438ffb2cd1188d4a35627c2bdfd578d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-90b26af068cbac8a94b0fe9cdfd7bc53e438ffb2cd1188d4a35627c2bdfd578d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-90b26af068cbac8a94b0fe9cdfd7bc53e438ffb2cd1188d4a35627c2bdfd578d|COMPLETE" crlf))
