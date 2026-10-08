(deffacts seed (old) (new))
(defrule absent-first (not (missing)) (new) => (printout t ABSENT-FIRST crlf))
(defrule fact-first (old) => (printout t FACT-FIRST crlf))
