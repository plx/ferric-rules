; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/router/resources/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-451cbe93bc83129461518379bbed8fbfc9dbaeeb9d4d6d6195486bfce8a590e4-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-451cbe93bc83129461518379bbed8fbfc9dbaeeb9d4d6d6195486bfce8a590e4|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-451cbe93bc83129461518379bbed8fbfc9dbaeeb9d4d6d6195486bfce8a590e4|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-451cbe93bc83129461518379bbed8fbfc9dbaeeb9d4d6d6195486bfce8a590e4|COMPLETE" crlf))
