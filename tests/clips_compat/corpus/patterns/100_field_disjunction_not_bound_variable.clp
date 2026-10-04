;; A variable alternative inside not is correlated with each outer key.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, join
(deffacts seed (key a) (key c) (sym a) (sym d))
(defrule match (key ?k) (not (sym ?k|b)) => (printout t "absent " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
