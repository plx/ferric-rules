; Harness for telefonica-clips/branches/64x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-ca3bcb9eafdcf800ae8342b309af509e57af8882fccd9b7e1e3dad36962962f5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3bcb9eafdcf800ae8342b309af509e57af8882fccd9b7e1e3dad36962962f5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3bcb9eafdcf800ae8342b309af509e57af8882fccd9b7e1e3dad36962962f5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-ca3bcb9eafdcf800ae8342b309af509e57af8882fccd9b7e1e3dad36962962f5|COMPLETE" crlf))
