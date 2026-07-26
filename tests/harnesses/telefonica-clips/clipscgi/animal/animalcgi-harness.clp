; Harness for telefonica-clips/clipscgi/animal/animalcgi.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-3dac5c4c6679a4ea35a1c804a67c515431ef0bad5f3ef987ba76b6f916f66244-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-3dac5c4c6679a4ea35a1c804a67c515431ef0bad5f3ef987ba76b6f916f66244|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3dac5c4c6679a4ea35a1c804a67c515431ef0bad5f3ef987ba76b6f916f66244|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3dac5c4c6679a4ea35a1c804a67c515431ef0bad5f3ef987ba76b6f916f66244|COMPLETE" crlf))
