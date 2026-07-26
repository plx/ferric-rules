; Harness for telefonica-clips/branches/64x/test_suite/globlerr.clp
; Detected constructs: defglobal: ?*x*, ?*r*, ?*y*, ?*z*, ?*w*, ?*q*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-8455d6dc2e819a0fdfcb0625afaca57a24d9660bfdca5f8fa8edc84b0bbb5a5f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-8455d6dc2e819a0fdfcb0625afaca57a24d9660bfdca5f8fa8edc84b0bbb5a5f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8455d6dc2e819a0fdfcb0625afaca57a24d9660bfdca5f8fa8edc84b0bbb5a5f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-8455d6dc2e819a0fdfcb0625afaca57a24d9660bfdca5f8fa8edc84b0bbb5a5f|COMPLETE" crlf))
