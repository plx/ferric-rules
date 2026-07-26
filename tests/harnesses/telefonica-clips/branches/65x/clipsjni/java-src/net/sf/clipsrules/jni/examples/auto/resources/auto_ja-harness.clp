; Harness for telefonica-clips/branches/65x/clipsjni/java-src/net/sf/clipsrules/jni/examples/auto/resources/auto_ja.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-f40077634659a19222a3918b7e9a99c83295fffc85eb1994df7a288749741836-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-f40077634659a19222a3918b7e9a99c83295fffc85eb1994df7a288749741836|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f40077634659a19222a3918b7e9a99c83295fffc85eb1994df7a288749741836|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f40077634659a19222a3918b7e9a99c83295fffc85eb1994df7a288749741836|COMPLETE" crlf))
