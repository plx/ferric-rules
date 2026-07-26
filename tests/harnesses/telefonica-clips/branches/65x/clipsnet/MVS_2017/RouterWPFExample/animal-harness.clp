; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterWPFExample/animal.clp
; Detected constructs: deffacts: MAIN::knowledge-base
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-d8f7a3588f51ce7c9516a1c1714d14da0222609d08f85cfec60f49f24ea8ca00-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-d8f7a3588f51ce7c9516a1c1714d14da0222609d08f85cfec60f49f24ea8ca00|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d8f7a3588f51ce7c9516a1c1714d14da0222609d08f85cfec60f49f24ea8ca00|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-d8f7a3588f51ce7c9516a1c1714d14da0222609d08f85cfec60f49f24ea8ca00|COMPLETE" crlf))
