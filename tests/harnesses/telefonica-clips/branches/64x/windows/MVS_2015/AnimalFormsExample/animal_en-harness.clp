; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AnimalFormsExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-fbc2b505571cc00771c105cde238dc0069eb08a26d2e5855a1340b59e7888271-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-fbc2b505571cc00771c105cde238dc0069eb08a26d2e5855a1340b59e7888271|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fbc2b505571cc00771c105cde238dc0069eb08a26d2e5855a1340b59e7888271|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-fbc2b505571cc00771c105cde238dc0069eb08a26d2e5855a1340b59e7888271|COMPLETE" crlf))
