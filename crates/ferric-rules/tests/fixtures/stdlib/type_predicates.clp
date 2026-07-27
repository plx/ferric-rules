; Type predicate functions: evenp, oddp, lexemep.
(deffacts startup (run-predicates))

(defrule predicates
    (run-predicates)
    =>
    (printout t "evenp-4: " (evenp 4) crlf)
    (printout t "evenp-3: " (evenp 3) crlf)
    (printout t "oddp-7: " (oddp 7) crlf)
    (printout t "oddp-6: " (oddp 6) crlf)
    (printout t "lexemep-sym: " (lexemep hello) crlf)
    (printout t "lexemep-int: " (lexemep 42) crlf))
