; Harness for clips-official/clipsjni/examples/AnimalDemo/animaldemo.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8663bd83ad86a6a40d166a79fb22977534156bc565b208769b5e76a5ff3d34b8-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8663bd83ad86a6a40d166a79fb22977534156bc565b208769b5e76a5ff3d34b8|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8663bd83ad86a6a40d166a79fb22977534156bc565b208769b5e76a5ff3d34b8|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8663bd83ad86a6a40d166a79fb22977534156bc565b208769b5e76a5ff3d34b8|COMPLETE" crlf))
