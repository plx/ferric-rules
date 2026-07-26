; Harness for clips-official/test_suite/gnrcovl.clp
; Detected constructs: defglobal: ?*success*; deffunction: alt-str-cat/1, print-result/2, testit/0; defmethod: sym-cat
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-16c6112b63cbbf90cda2c1406165904e883b9407c51d222550a1dc60461b5b88-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-16c6112b63cbbf90cda2c1406165904e883b9407c51d222550a1dc60461b5b88|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-16c6112b63cbbf90cda2c1406165904e883b9407c51d222550a1dc60461b5b88|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-16c6112b63cbbf90cda2c1406165904e883b9407c51d222550a1dc60461b5b88|COMPLETE" crlf))
