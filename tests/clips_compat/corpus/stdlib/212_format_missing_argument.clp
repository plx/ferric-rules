;; format requires exactly one argument per directive: too few is an error.
;; Level: boundary
;; Covers: format
(defrule exact (declare (salience 10)) =>
  (printout t (format nil "%d %d|" 1 2) crlf))
(defrule missing =>
  (bind ?result (format nil "%d %d|" 1))
  (printout t "not reached " ?result crlf))
