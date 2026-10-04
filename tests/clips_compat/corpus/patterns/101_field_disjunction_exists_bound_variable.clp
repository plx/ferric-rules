;; Overlapping variable and literal witnesses satisfy each outer key once.
;; Level: interaction
;; Covers: patterns, field-disjunction, exists, join
(deffacts seed (key a) (key c) (sym a) (sym b) (sym d))
(defrule match (key ?k) (exists (sym ?k|b)) => (printout t "present " ?k crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
