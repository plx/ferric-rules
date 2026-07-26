; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/animal/resources/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-75e54816625435cd850d26097111d9ba21d7e4a93f76497e02591261ca137a21-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-75e54816625435cd850d26097111d9ba21d7e4a93f76497e02591261ca137a21|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-75e54816625435cd850d26097111d9ba21d7e4a93f76497e02591261ca137a21|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-75e54816625435cd850d26097111d9ba21d7e4a93f76497e02591261ca137a21|COMPLETE" crlf))
