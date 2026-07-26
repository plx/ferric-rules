; Harness for clips-official/test_suite/bigbug.clp
; Detected constructs: defglobal: ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-10cad3dd19937df737ecca8944da0dffe720fe3ef92cb245f6e36d6cc44f257e-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-10cad3dd19937df737ecca8944da0dffe720fe3ef92cb245f6e36d6cc44f257e|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-10cad3dd19937df737ecca8944da0dffe720fe3ef92cb245f6e36d6cc44f257e|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-10cad3dd19937df737ecca8944da0dffe720fe3ef92cb245f6e36d6cc44f257e|COMPLETE" crlf))
