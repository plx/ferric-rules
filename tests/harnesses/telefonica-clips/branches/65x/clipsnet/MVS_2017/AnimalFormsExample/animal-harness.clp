; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AnimalFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-7c5fa5b27ddfdd4c22385b33993088129bc332f26296c7c01a3aa0645e70161d-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c5fa5b27ddfdd4c22385b33993088129bc332f26296c7c01a3aa0645e70161d|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c5fa5b27ddfdd4c22385b33993088129bc332f26296c7c01a3aa0645e70161d|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-7c5fa5b27ddfdd4c22385b33993088129bc332f26296c7c01a3aa0645e70161d|COMPLETE" crlf))
