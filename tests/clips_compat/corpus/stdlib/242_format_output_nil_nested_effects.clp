;; Format nil suppresses its own write while preserving argument output and return value.
;; Level: interaction
;; Covers: format, printout, deffunction
(defrule probe =>
  (printout t "prefix " (format nil "outer[%s]" (format t "inner%n")) " suffix" crlf)
  (bind ?result (format nil "silent %d" 7))
  (printout t "returned: [" ?result "]" crlf))
