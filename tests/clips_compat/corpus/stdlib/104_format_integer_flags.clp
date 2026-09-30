;; #340 pinned CLIPS characterization: integer-flags

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%-05d" 7) "]" crlf)
(printout t "[" (format nil "%-05d" -7) "]" crlf)
(printout t "[" (format nil "%0-5d" 7) "]" crlf)
(printout t "[" (format nil "%0-5d" -7) "]" crlf)
(printout t "[" (format nil "%0005d" 7) "]" crlf)
(printout t "[" (format nil "%0005d" -7) "]" crlf)
)
