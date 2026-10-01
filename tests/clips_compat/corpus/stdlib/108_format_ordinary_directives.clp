;; #340 pinned CLIPS characterization: ordinary-directives

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%6d|%-6d|%6.2f|%6s|%-6s|%.3s|%06s" 7 7 2.5 "red" red "abcdef" red) "]" crlf)
(printout t "[" (format nil "%%:%n:%r:%t:%v" ) "]" crlf)
)
