;; A deffunction can return the same formatted string that it writes.
;; Level: interaction
;; Covers: format, printout, deffunction
(deffunction render (?x) (format t "value=%d" ?x))
(defrule probe => (printout t "<" (render 7) ">" crlf))
