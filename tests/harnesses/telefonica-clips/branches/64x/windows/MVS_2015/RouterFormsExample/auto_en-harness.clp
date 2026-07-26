; Harness for telefonica-clips/branches/64x/windows/MVS_2015/RouterFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-db576e5d90c301810deb4a32d1bfcc303730f97dec107d141e1c64efcd245f69-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-db576e5d90c301810deb4a32d1bfcc303730f97dec107d141e1c64efcd245f69|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-db576e5d90c301810deb4a32d1bfcc303730f97dec107d141e1c64efcd245f69|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-db576e5d90c301810deb4a32d1bfcc303730f97dec107d141e1c64efcd245f69|COMPLETE" crlf))
