; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-75bebd33732d04f9e6e11f158e9890ca9b0fe4dd454e1ac84519f6e7d38e867f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-75bebd33732d04f9e6e11f158e9890ca9b0fe4dd454e1ac84519f6e7d38e867f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-75bebd33732d04f9e6e11f158e9890ca9b0fe4dd454e1ac84519f6e7d38e867f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-75bebd33732d04f9e6e11f158e9890ca9b0fe4dd454e1ac84519f6e7d38e867f|COMPLETE" crlf))
