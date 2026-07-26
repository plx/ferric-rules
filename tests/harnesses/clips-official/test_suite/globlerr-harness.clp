; Harness for clips-official/test_suite/globlerr.clp
; Detected constructs: defglobal: ?*x*, ?*r*, ?*y*, ?*z*, ?*w*, ?*q*
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-0694d8a845debeed867fb7df60ca10c65fc670658d41f4c0f557dcc959ac6194-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-0694d8a845debeed867fb7df60ca10c65fc670658d41f4c0f557dcc959ac6194|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0694d8a845debeed867fb7df60ca10c65fc670658d41f4c0f557dcc959ac6194|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-0694d8a845debeed867fb7df60ca10c65fc670658d41f4c0f557dcc959ac6194|COMPLETE" crlf))
