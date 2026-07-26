; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterFormsExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-42b81f5fc8c990d6a9cf5081b8e295a763908cdbd3991e8cc796dae1a667c4f1-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-42b81f5fc8c990d6a9cf5081b8e295a763908cdbd3991e8cc796dae1a667c4f1|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-42b81f5fc8c990d6a9cf5081b8e295a763908cdbd3991e8cc796dae1a667c4f1|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-42b81f5fc8c990d6a9cf5081b8e295a763908cdbd3991e8cc796dae1a667c4f1|COMPLETE" crlf))
