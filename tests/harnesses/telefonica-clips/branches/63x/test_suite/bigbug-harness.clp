; Harness for telefonica-clips/branches/63x/test_suite/bigbug.clp
; Detected constructs: defglobal: ?*x*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-245659c6a3168b5776daabfac0774d00b3145bd18d16fdb08f53f77f36e742ea-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-245659c6a3168b5776daabfac0774d00b3145bd18d16fdb08f53f77f36e742ea|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-245659c6a3168b5776daabfac0774d00b3145bd18d16fdb08f53f77f36e742ea|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-245659c6a3168b5776daabfac0774d00b3145bd18d16fdb08f53f77f36e742ea|COMPLETE" crlf))
