; Harness for galletas/bh.clp
; Detected constructs: deffacts: hechos_iniciales
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-3cb67d26decdbf3f334245bed02bbbc4ea8fceee0b7978d87620a3e40b092f25-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-3cb67d26decdbf3f334245bed02bbbc4ea8fceee0b7978d87620a3e40b092f25|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3cb67d26decdbf3f334245bed02bbbc4ea8fceee0b7978d87620a3e40b092f25|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-3cb67d26decdbf3f334245bed02bbbc4ea8fceee0b7978d87620a3e40b092f25|COMPLETE" crlf))
