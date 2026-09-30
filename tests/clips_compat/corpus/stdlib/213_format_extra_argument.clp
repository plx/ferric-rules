;; format requires exactly one argument per directive: too many is an error.
;; Level: boundary
;; Covers: format
(defrule exact (declare (salience 10)) =>
  (printout t (format nil "%d|" 1) crlf))
(defrule extra =>
  (bind ?result (format nil "%d|" 1 2))
  (printout t "not reached " ?result crlf))
