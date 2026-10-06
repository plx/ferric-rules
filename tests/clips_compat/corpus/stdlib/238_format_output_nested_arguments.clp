;; Inner format output precedes outer format output and its returned string.
;; Level: interaction
;; Covers: format, printout, deffunction
(defrule probe =>
  (printout t "prefix " (format t "outer[%s]%n" (format t "inner%n")) " suffix" crlf))
